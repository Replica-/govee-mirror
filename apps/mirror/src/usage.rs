//! The CPU and GPU share this process uses, as Task Manager counts them.
//!
//! The GPU share comes from the "GPU Engine" performance counters, through
//! PDH. A Win32 call needs `unsafe`: each call takes a query this module owns
//! and a buffer that outlives it.

use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::Duration;

use crate::capture;

const PERIOD: Duration = Duration::from_secs(1);

/// Percent of the whole machine: 100 is every core busy, or one GPU engine
/// busy.
#[derive(Debug, Clone, Copy, Default)]
pub struct Usage {
    pub cpu: f32,
    /// `None` where the counters are not available.
    pub gpu: Option<f32>,
}

pub type Shared = Arc<Mutex<Option<Usage>>>;

pub fn lock(shared: &Shared) -> std::sync::MutexGuard<'_, Option<Usage>> {
    shared.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Sample once a second while the window shows. The GPU counters cost a few
/// milliseconds of CPU per sample, so a hidden window stops them.
pub fn spawn(capture: capture::Shared) -> Shared {
    let shared: Shared = Arc::default();
    let out = Arc::clone(&shared);
    let _ = thread::Builder::new()
        .name("usage".to_owned())
        .spawn(move || {
            let mut sampler: Option<Sampler> = None;
            loop {
                if capture::lock(&capture).demand.window {
                    let sampler = sampler.get_or_insert_with(Sampler::new);
                    *lock(&out) = sampler.sample();
                } else {
                    // The next sample measures from when the window shows.
                    sampler = None;
                    *lock(&out) = None;
                }
                thread::sleep(PERIOD);
            }
        });
    shared
}

#[cfg(windows)]
use self::windows_impl::Sampler;

#[cfg(windows)]
#[allow(unsafe_code)]
mod windows_impl {
    use std::collections::HashMap;
    use std::time::Instant;

    use windows::Win32::Foundation::FILETIME;
    use windows::Win32::System::Performance::{
        PDH_CSTATUS_VALID_DATA, PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE, PDH_HCOUNTER,
        PDH_HQUERY, PDH_MORE_DATA, PDH_NO_DATA, PdhAddEnglishCounterW, PdhCloseQuery,
        PdhCollectQueryData, PdhGetFormattedCounterArrayW, PdhOpenQueryW,
    };
    use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};
    use windows::core::{HSTRING, PCWSTR};

    use super::Usage;

    pub struct Sampler {
        cores: f32,
        /// Kernel plus user time in 100 ns units, and when it was read.
        last: Option<(u64, Instant)>,
        gpu: Option<(PDH_HQUERY, PDH_HCOUNTER)>,
    }

    // SAFETY: a PDH query has no thread affinity, and one thread owns it.
    unsafe impl Send for Sampler {}

    impl Sampler {
        pub fn new() -> Self {
            #[allow(clippy::cast_precision_loss)]
            let cores = std::thread::available_parallelism().map_or(1, usize::from) as f32;
            Self {
                cores,
                last: None,
                gpu: open_gpu(),
            }
        }

        /// `None` on the first call: a share needs two readings.
        pub fn sample(&mut self) -> Option<Usage> {
            let now = (cpu_time()?, Instant::now());
            let last = self.last.replace(now)?;
            #[allow(clippy::cast_precision_loss)]
            let busy = now.0.saturating_sub(last.0) as f32 / 1e7;
            let wall = now.1.duration_since(last.1).as_secs_f32().max(f32::EPSILON);
            Some(Usage {
                cpu: 100.0 * busy / wall / self.cores,
                gpu: self.gpu.and_then(|(query, counter)| gpu(query, counter)),
            })
        }
    }

    impl Drop for Sampler {
        fn drop(&mut self) {
            if let Some((query, _)) = self.gpu {
                unsafe { PdhCloseQuery(query) };
            }
        }
    }

    fn cpu_time() -> Option<u64> {
        let mut times = [FILETIME::default(); 4];
        let [created, exited, kernel, user] = &mut times;
        unsafe { GetProcessTimes(GetCurrentProcess(), created, exited, kernel, user) }.ok()?;
        let ticks = |t: &FILETIME| u64::from(t.dwHighDateTime) << 32 | u64::from(t.dwLowDateTime);
        Some(ticks(kernel) + ticks(user))
    }

    /// A query on every GPU engine of this process. The counter is a rate, so
    /// it reads nothing until the second collect.
    fn open_gpu() -> Option<(PDH_HQUERY, PDH_HCOUNTER)> {
        let path = HSTRING::from(format!(
            r"\GPU Engine(pid_{}_*)\Utilization Percentage",
            std::process::id()
        ));
        let mut query = PDH_HQUERY::default();
        if unsafe { PdhOpenQueryW(PCWSTR::null(), 0, &mut query) } != 0 {
            return None;
        }
        let mut counter = PDH_HCOUNTER::default();
        if unsafe { PdhAddEnglishCounterW(query, &path, 0, &mut counter) } != 0 {
            unsafe { PdhCloseQuery(query) };
            return None;
        }
        // `PDH_NO_DATA` here means no engine has done work yet.
        let _ = unsafe { PdhCollectQueryData(query) };
        Some((query, counter))
    }

    /// The busiest engine type, with the engines of one type added up.
    fn gpu(query: PDH_HQUERY, counter: PDH_HCOUNTER) -> Option<f32> {
        match unsafe { PdhCollectQueryData(query) } {
            0 => {}
            PDH_NO_DATA => return Some(0.0),
            _ => return None,
        }
        let (mut size, mut count) = (0u32, 0u32);
        let status = unsafe {
            PdhGetFormattedCounterArrayW(counter, PDH_FMT_DOUBLE, &mut size, &mut count, None)
        };
        if status != PDH_MORE_DATA {
            // No engine of this process has done work yet.
            return Some(0.0);
        }
        // The names point into the buffer, after the items.
        let item = size_of::<PDH_FMT_COUNTERVALUE_ITEM_W>();
        let mut items =
            vec![PDH_FMT_COUNTERVALUE_ITEM_W::default(); (size as usize).div_ceil(item)];
        let status = unsafe {
            PdhGetFormattedCounterArrayW(
                counter,
                PDH_FMT_DOUBLE,
                &mut size,
                &mut count,
                Some(items.as_mut_ptr()),
            )
        };
        if status != 0 {
            return None;
        }
        let mut by_type: HashMap<String, f64> = HashMap::new();
        for item in items.iter().take(count as usize) {
            if item.FmtValue.CStatus != PDH_CSTATUS_VALID_DATA {
                continue;
            }
            let name = unsafe { item.szName.to_string() }.unwrap_or_default();
            let kind = name.rsplit_once("engtype_").map_or("", |(_, t)| t);
            *by_type.entry(kind.to_owned()).or_default() +=
                unsafe { item.FmtValue.Anonymous.doubleValue };
        }
        #[allow(clippy::cast_possible_truncation)]
        Some(by_type.into_values().fold(0.0, f64::max) as f32)
    }
}

#[cfg(not(windows))]
struct Sampler;

#[cfg(not(windows))]
impl Sampler {
    fn new() -> Self {
        Self
    }

    fn sample(&mut self) -> Option<Usage> {
        None
    }
}
