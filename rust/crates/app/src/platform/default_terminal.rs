//! "Make Angelly the Default Terminal": registers the app as the handler for shell
//! scripts and Unix executables, the content types Terminal.app owns by default.

#[cfg(target_os = "macos")]
pub fn make_default() {
    use objc2_foundation::{NSBundle, NSString};

    const SHELL_SCRIPT: &str = "com.apple.terminal.shell-script";
    const UNIX_EXECUTABLE: &str = "public.unix-executable";
    const ROLE_ALL: u32 = 0xFFFF_FFFF;

    #[link(name = "CoreServices", kind = "framework")]
    unsafe extern "C" {
        fn LSSetDefaultRoleHandlerForContentType(
            content_type: *const NSString,
            role: u32,
            handler_bundle_id: *const NSString,
        ) -> i32;
    }

    let Some(id) = NSBundle::mainBundle().bundleIdentifier() else { return };
    for content_type in [SHELL_SCRIPT, UNIX_EXECUTABLE] {
        let content_type = NSString::from_str(content_type);
        unsafe { LSSetDefaultRoleHandlerForContentType(&*content_type, ROLE_ALL, &*id) };
    }
}

#[cfg(not(target_os = "macos"))]
pub fn make_default() {}
