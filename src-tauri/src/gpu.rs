//! GPU utilization, read from the "PerformanceStatistics" that each IOKit
//! IOAccelerator (one per GPU) publishes. This is the figure Activity Monitor's
//! GPU History shows, and reading it needs no special permissions.

/// Busiest GPU's "Device Utilization %", 0..100; None if no GPU reports one.
#[cfg(target_os = "macos")]
pub fn utilization() -> Option<f32> {
    use std::ffi::{c_char, c_void};

    type CFRef = *const c_void;

    #[link(name = "IOKit", kind = "framework")]
    unsafe extern "C" {
        fn IOServiceMatching(name: *const c_char) -> *mut c_void;
        fn IOServiceGetMatchingServices(main_port: u32, matching: *mut c_void, iter: *mut u32) -> i32;
        fn IOIteratorNext(iter: u32) -> u32;
        fn IORegistryEntryCreateCFProperties(entry: u32, props: *mut CFRef, alloc: CFRef, options: u32) -> i32;
        fn IOObjectRelease(obj: u32) -> i32;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFStringCreateWithCString(alloc: CFRef, s: *const c_char, encoding: u32) -> CFRef;
        fn CFDictionaryGetValue(dict: CFRef, key: CFRef) -> CFRef;
        fn CFGetTypeID(cf: CFRef) -> usize;
        fn CFDictionaryGetTypeID() -> usize;
        fn CFNumberGetTypeID() -> usize;
        fn CFNumberGetValue(number: CFRef, kind: isize, out: *mut c_void) -> bool;
        fn CFRelease(cf: CFRef);
    }

    const UTF8: u32 = 0x0800_0100;
    const SINT64: isize = 4;

    unsafe {
        // The matching dictionary is consumed by IOServiceGetMatchingServices.
        let matching = IOServiceMatching(c"IOAccelerator".as_ptr());
        if matching.is_null() {
            return None;
        }
        let mut iter = 0u32;
        // Port 0 is kIOMainPortDefault.
        if IOServiceGetMatchingServices(0, matching, &mut iter) != 0 {
            return None;
        }
        let stats_key = CFStringCreateWithCString(std::ptr::null(), c"PerformanceStatistics".as_ptr(), UTF8);
        let util_key = CFStringCreateWithCString(std::ptr::null(), c"Device Utilization %".as_ptr(), UTF8);

        let mut busiest: Option<f32> = None;
        loop {
            let service = IOIteratorNext(iter);
            if service == 0 {
                break;
            }
            let mut props: CFRef = std::ptr::null();
            if IORegistryEntryCreateCFProperties(service, &mut props, std::ptr::null(), 0) == 0 && !props.is_null() {
                let stats = CFDictionaryGetValue(props, stats_key);
                if !stats.is_null() && CFGetTypeID(stats) == CFDictionaryGetTypeID() {
                    let util = CFDictionaryGetValue(stats, util_key);
                    let mut value = 0i64;
                    if !util.is_null()
                        && CFGetTypeID(util) == CFNumberGetTypeID()
                        && CFNumberGetValue(util, SINT64, &mut value as *mut i64 as *mut c_void)
                    {
                        let pct = (value as f32).clamp(0.0, 100.0);
                        busiest = Some(busiest.map_or(pct, |b| b.max(pct)));
                    }
                }
                CFRelease(props);
            }
            IOObjectRelease(service);
        }

        IOObjectRelease(iter);
        CFRelease(stats_key);
        CFRelease(util_key);
        busiest
    }
}

#[cfg(not(target_os = "macos"))]
pub fn utilization() -> Option<f32> {
    None
}
