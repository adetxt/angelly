//! A tiny wrapper over `NSNotificationCenter` block observers.

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2_foundation::{NSNotification, NSNotificationCenter, NSNotificationName, NSObjectProtocol};
use std::ptr::NonNull;

/// Keeps the observer registered for as long as it lives.
pub struct Observer {
    token: Retained<ProtocolObject<dyn NSObjectProtocol>>,
    center: Retained<NSNotificationCenter>,
}

impl Drop for Observer {
    fn drop(&mut self) {
        unsafe { self.center.removeObserver(AsRef::<AnyObject>::as_ref(&*self.token)) };
    }
}

/// Calls `handler` on the main thread each time `name` is posted by the default center.
pub fn observe(name: &NSNotificationName, handler: impl Fn() + 'static) -> Observer {
    let center = NSNotificationCenter::defaultCenter();
    let block = RcBlock::new(move |_: NonNull<NSNotification>| handler());
    let token = unsafe { center.addObserverForName_object_queue_usingBlock(Some(name), None::<&AnyObject>, None, &block) };
    Observer { token, center }
}
