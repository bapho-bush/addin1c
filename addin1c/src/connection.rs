use std::ffi::{c_int, c_long, c_ushort, c_void};

use crate::{tvariant::TVariant, CStr1C};

#[repr(C)]
struct ConnectionVTable {
    dtor: usize,
    #[cfg(target_family = "unix")]
    dtor2: usize,
    add_error:
        unsafe extern "system" fn(&Connection, c_ushort, *const u16, *const u16, c_long) -> bool,
    read: unsafe extern "system" fn(
        &Connection,
        *mut u16,
        &mut TVariant,
        c_long,
        *mut *mut u16,
    ) -> bool,
    write: unsafe extern "system" fn(&Connection, *mut u16, &mut TVariant) -> bool,
    register_profile_as: unsafe extern "system" fn(&Connection, *mut u16) -> bool,
    set_event_buffer_depth: unsafe extern "system" fn(&Connection, c_long) -> bool,
    get_event_buffer_depth: unsafe extern "system" fn(&Connection) -> c_long,
    external_event:
        unsafe extern "system" fn(&Connection, *const u16, *const u16, *const u16) -> bool,
    clean_event_buffer: unsafe extern "system" fn(&Connection),
    set_status_line: unsafe extern "system" fn(&Connection, *mut u16) -> bool,
    reset_status_line: unsafe extern "system" fn(&Connection),
    // IAddInDefBaseEx, enabled only after SetPlatformCapabilities >= 1.
    get_interface: unsafe extern "system" fn(&Connection, c_int) -> *const c_void,
}

#[cfg(test)]
mod attached_info_tests {
    use super::*;
    use std::{
        mem, ptr,
        sync::atomic::{AtomicI32, Ordering},
    };

    #[repr(C)]
    struct MockAttached {
        vptr: *const AttachedInfoVTable,
        mode: AtomicI32,
    }
    #[repr(C)]
    struct MockConnection {
        connection: Connection,
        attached: *const c_void,
    }
    unsafe extern "system" fn get_interface(c: &Connection, interface: c_int) -> *const c_void {
        assert_eq!(interface, 2);
        (*(c as *const Connection as *const MockConnection)).attached
    }
    unsafe extern "system" fn get_mode(a: *const AttachedInfo) -> c_int {
        (*(a as *const MockAttached)).mode.load(Ordering::Relaxed)
    }

    #[test]
    fn actual_attached_state_or_unavailable() {
        // Only GetInterface is ever called on this mock; unused vtable members
        // use zeroed placeholders in MaybeUninit, never read as function pointers.
        let mut table = mem::MaybeUninit::<ConnectionVTable>::zeroed();
        unsafe {
            ptr::addr_of_mut!((*table.as_mut_ptr()).get_interface).write(get_interface);
            let attached_table = AttachedInfoVTable {
                get_attached_info: get_mode,
            };
            let attached = MockAttached {
                vptr: &attached_table,
                mode: AtomicI32::new(0),
            };
            // Connection stores a reference to a complete vtable; unused members
            // must still be valid function pointer representations. Populate each
            // from a non-null function address before creating that reference.
            let words = table.as_mut_ptr().cast::<usize>();
            for i in 0..(mem::size_of::<ConnectionVTable>() / mem::size_of::<usize>()) {
                if words.add(i).read() == 0 {
                    words.add(i).write(get_interface as *const () as usize);
                }
            }
            let connection = MockConnection {
                connection: Connection {
                    vptr1: &*table.as_ptr(),
                },
                attached: {
                    let pointer = (&attached as *const MockAttached).cast::<u8>();
                    #[cfg(target_env = "msvc")]
                    let pointer = pointer.add(mem::size_of::<*const ()>());
                    pointer.cast()
                },
            };
            assert_eq!(connection.connection.is_attached_isolated(), Some(true));
            attached.mode.store(1, Ordering::Relaxed);
            assert_eq!(connection.connection.is_attached_isolated(), Some(false));
            attached.mode.store(99, Ordering::Relaxed);
            assert_eq!(connection.connection.is_attached_isolated(), None);
            let unavailable = MockConnection {
                connection: Connection {
                    vptr1: &*table.as_ptr(),
                },
                attached: ptr::null(),
            };
            assert_eq!(unavailable.connection.is_attached_isolated(), None);
        }
    }
}

#[repr(C)]
struct AttachedInfoVTable {
    get_attached_info: unsafe extern "system" fn(*const AttachedInfo) -> c_int,
}

#[repr(C)]
struct AttachedInfo {
    vptr: *const AttachedInfoVTable,
}

#[repr(C)]
pub struct Connection {
    vptr1: &'static ConnectionVTable,
}

impl Connection {
    /// Query the platform's actual attached mode via SDK IAttachedInfo.
    ///
    /// # Safety
    /// Call only after SetPlatformCapabilities advertised extended interfaces
    /// with a value >= 1. The platform connection must still be live.
    pub unsafe fn is_attached_isolated(&self) -> Option<bool> {
        // eIAttachedInfo = 2 on desktop targets; Android is outside project scope.
        let base = (self.vptr1.get_interface)(self, 2);
        if base.is_null() {
            return None;
        }
        // SDK returns IInterface*, not IAttachedInfo*. MSVC places its empty
        // IInterface base after the derived vptr; this mirrors C++ static_cast.
        #[cfg(target_env = "msvc")]
        let interface =
            base.cast::<u8>().sub(std::mem::size_of::<*const ()>()) as *const AttachedInfo;
        #[cfg(not(target_env = "msvc"))]
        let interface = base as *const AttachedInfo;
        if (*interface).vptr.is_null() {
            return None;
        }
        match ((*(*interface).vptr).get_attached_info)(interface) {
            0 => Some(true),
            1 => Some(false),
            _ => None,
        }
    }
    pub fn external_event(
        &self,
        source: impl AsRef<CStr1C>,
        message: impl AsRef<CStr1C>,
        data: impl AsRef<CStr1C>,
    ) -> bool {
        unsafe {
            (self.vptr1.external_event)(
                self,
                source.as_ref().as_ptr(),
                message.as_ref().as_ptr(),
                data.as_ref().as_ptr(),
            )
        }
    }

    pub fn set_event_buffer_depth(&self, depth: c_long) -> bool {
        unsafe { (self.vptr1.set_event_buffer_depth)(self, depth) }
    }

    pub fn get_event_buffer_depth(&self) -> c_long {
        unsafe { (self.vptr1.get_event_buffer_depth)(self) }
    }

    pub fn clean_event_buffer(&self) {
        unsafe { (self.vptr1.clean_event_buffer)(self) }
    }
}
