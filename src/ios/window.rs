//! iOS Window implementation using UIWindow and UIViewController.
//!
//! iOS windows are fundamentally different from desktop windows:
//! - Always fullscreen (or split-screen on iPad)
//! - No title bar or window chrome
//! - Touch-based input
//! - Safe area insets for notch/home indicator
//!
//! The window is backed by a UIWindow containing a UIViewController
//! whose view hosts a CAMetalLayer. Rendering is performed by
//! `gpui_wgpu::WgpuRenderer` which drives wgpu over the Metal backend.

use super::events::*;
use super::IosDisplay;
use crate::momentum::{MomentumScroller, VelocityTracker};
use gpui::{
    point, px, size, AnyWindowHandle, AtlasKey, AtlasTextureId, AtlasTextureKind, AtlasTile,
    Bounds, Capslock, DevicePixels, DispatchEventResult, GpuSpecs, Modifiers, Pixels,
    PlatformAtlas, PlatformDisplay, PlatformInput, PlatformInputHandler, PlatformWindow, Point,
    PromptButton, PromptLevel, RequestFrameOptions, Scene, Size, TileId, WindowAppearance,
    WindowBackgroundAppearance, WindowBounds, WindowControlArea, WindowParams,
};
use gpui_wgpu::{GpuContext, WgpuContext, WgpuRenderer, WgpuSurfaceConfig};
use objc2::encode::{Encode, Encoding, RefEncode};
use objc2::runtime::{AnyClass, AnyObject, Bool, ClassBuilder, Sel};
use objc2::{class, msg_send, sel};

use super::cg_types::{ObjcCGPoint, ObjcCGRect};
use parking_lot::Mutex;
use raw_window_handle::{HasDisplayHandle, HasWindowHandle, UiKitDisplayHandle, UiKitWindowHandle};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    ffi::c_void,
    ptr::{self, NonNull},
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant},
};

const GPUI_WINDOW_IVAR: &str = "gpui_window_ptr";
const KEY_REPEAT_DELAY: Duration = Duration::from_millis(500);
const KEY_REPEAT_INTERVAL: Duration = Duration::from_millis(60);

#[derive(Clone)]
enum HardwareKeyAction {
    Text(String),
    Delete,
    Key {
        code: u32,
        flags: u32,
        characters: Option<String>,
    },
}

struct HeldHardwareKey {
    action: HardwareKeyAction,
    next_repeat: Instant,
}

/// Lightweight window handle for wgpu surface creation.
/// Stores the raw UIView pointer needed by wgpu to create a Metal surface.
/// Implements the traits required by `WgpuRenderer::new`.
#[derive(Debug, Clone, Copy)]
struct RawIosWindow {
    view: *mut c_void,
}

unsafe impl Send for RawIosWindow {}
unsafe impl Sync for RawIosWindow {}

impl HasWindowHandle for RawIosWindow {
    fn window_handle(
        &self,
    ) -> std::result::Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError>
    {
        let view = NonNull::new(self.view).ok_or(raw_window_handle::HandleError::Unavailable)?;
        let handle = UiKitWindowHandle::new(view);
        Ok(unsafe { raw_window_handle::WindowHandle::borrow_raw(handle.into()) })
    }
}

impl HasDisplayHandle for RawIosWindow {
    fn display_handle(
        &self,
    ) -> std::result::Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError>
    {
        let handle = UiKitDisplayHandle::new();
        Ok(unsafe { raw_window_handle::DisplayHandle::borrow_raw(handle.into()) })
    }
}

static METAL_VIEW_CLASS_REGISTERED: std::sync::Once = std::sync::Once::new();
static VC_CLASS_REGISTERED: std::sync::Once = std::sync::Once::new();
static TEXT_INPUT_VIEW_CLASS_REGISTERED: std::sync::Once = std::sync::Once::new();

/// Global storage for the current status bar style.
/// 0 = default (dark content), 1 = light content.
/// Accessed from the main thread only.
static STATUS_BAR_STYLE: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

/// Register a custom UIViewController subclass that allows overriding
/// `preferredStatusBarStyle` at runtime.
fn register_view_controller_class() -> &'static AnyClass {
    VC_CLASS_REGISTERED.call_once(|| {
        let superclass = class!(UIViewController);
        let mut decl = ClassBuilder::new(c"GPUIViewController", superclass).unwrap();

        // Override preferredStatusBarStyle
        extern "C" fn preferred_status_bar_style(_this: *mut AnyObject, _sel: Sel) -> isize {
            let style = STATUS_BAR_STYLE.load(std::sync::atomic::Ordering::Relaxed);
            if style == 1 {
                1 // UIStatusBarStyleLightContent
            } else {
                3 // UIStatusBarStyleDarkContent (iOS 13+)
            }
        }

        // Override viewDidLayoutSubviews — called by UIKit on rotation,
        // split-screen changes, and any other layout pass.
        extern "C" fn view_did_layout_subviews(this: *mut AnyObject, _sel: Sel) {
            // Call super
            unsafe {
                let superclass = class!(UIViewController);
                let _: () = msg_send![super(this, superclass), viewDidLayoutSubviews];
            }

            // Notify all registered GPUI windows about the layout change.
            if let Some(wrapper) = super::ffi::IOS_WINDOW_LIST.get() {
                unsafe {
                    let windows = &*wrapper.0.get();
                    for &window_ptr in windows.iter() {
                        if !window_ptr.is_null() {
                            let window = &*window_ptr;
                            window.handle_layout_change();
                        }
                    }
                }
            }
        }

        extern "C" fn appearance_changed(this: *mut AnyObject, _sel: Sel) {
            log::info!("iZed appearance: UIKit trait callback");
            notify_appearance_changed(this);
        }

        // iOS 16 fallback. Newer systems use the trait registration below.
        extern "C" fn trait_collection_did_change(
            this: *mut AnyObject,
            _sel: Sel,
            previous: *mut AnyObject,
        ) {
            unsafe {
                let superclass = class!(UIViewController);
                let _: () = msg_send![super(this, superclass), traitCollectionDidChange: previous];
                let supports_trait_registration: Bool = msg_send![
                    this,
                    respondsToSelector: sel!(registerForTraitChanges:withAction:)
                ];
                if supports_trait_registration.as_bool() {
                    return;
                }
                let current: *mut AnyObject = msg_send![this, traitCollection];
                let current_style: isize = msg_send![current, userInterfaceStyle];
                let previous_style: isize = if previous.is_null() {
                    0
                } else {
                    msg_send![previous, userInterfaceStyle]
                };
                if current_style != previous_style {
                    notify_appearance_changed(this);
                }
            }
        }

        unsafe {
            decl.add_method(
                sel!(preferredStatusBarStyle),
                preferred_status_bar_style as extern "C" fn(*mut AnyObject, Sel) -> isize,
            );
            decl.add_method(
                sel!(viewDidLayoutSubviews),
                view_did_layout_subviews as extern "C" fn(*mut AnyObject, Sel),
            );
            decl.add_method(
                sel!(appearanceChanged),
                appearance_changed as extern "C" fn(*mut AnyObject, Sel),
            );
            decl.add_method(
                sel!(traitCollectionDidChange:),
                trait_collection_did_change
                    as extern "C" fn(*mut AnyObject, Sel, *mut AnyObject),
            );
        }

        decl.register();
    });

    class!(GPUIViewController)
}

fn notify_appearance_changed(view_controller: *mut AnyObject) {
    log::info!("iZed appearance: forwarding UIKit change");
    if let Some(wrapper) = super::ffi::IOS_WINDOW_LIST.get() {
        unsafe {
            for &window_ptr in (&*wrapper.0.get()).iter() {
                if !window_ptr.is_null() {
                    let window = &*window_ptr;
                    if window.view_controller == view_controller {
                        let controller_traits: *mut AnyObject = msg_send![view_controller, traitCollection];
                        let controller_style: isize = msg_send![controller_traits, userInterfaceStyle];
                        let view_traits: *mut AnyObject = msg_send![window.view, traitCollection];
                        let view_style: isize = msg_send![view_traits, userInterfaceStyle];
                        log::info!("iZed appearance: controller={controller_style}, view={view_style}, GPUI callback present={}", window.appearance_changed_callback.borrow().is_some());
                        if let Some(callback) = window.appearance_changed_callback.borrow_mut().as_mut() {
                            callback();
                        }
                    }
                }
            }
        }
    }
}

/// Set the iOS status bar content style (light or dark text/icons).
///
/// This updates the stored style and asks the root view controller
/// to re-query `preferredStatusBarStyle`.
pub fn set_status_bar_style(style: crate::StatusBarContentStyle) {
    use crate::StatusBarContentStyle;

    let value = match style {
        StatusBarContentStyle::Light => 1,
        StatusBarContentStyle::Dark => 0,
    };
    STATUS_BAR_STYLE.store(value, std::sync::atomic::Ordering::Relaxed);

    // Ask UIKit to re-query the status bar style
    unsafe {
        if let Some(wrapper) = super::ffi::IOS_WINDOW_LIST.get() {
            let windows = &*wrapper.0.get();
            if let Some(&window_ptr) = windows.last() {
                if !window_ptr.is_null() {
                    let window = &*window_ptr;
                    let vc = window.view_controller;
                    if !vc.is_null() {
                        let _: () = msg_send![vc, setNeedsStatusBarAppearanceUpdate];
                    }
                }
            }
        }
    }
}

/// Register a custom UIView subclass that uses CAMetalLayer as its backing layer.
/// This is required for Metal rendering on iOS.
fn register_metal_view_class() -> &'static AnyClass {
    METAL_VIEW_CLASS_REGISTERED.call_once(|| {
        let superclass = class!(UIView);
        let mut decl = ClassBuilder::new(c"GPUIMetalView", superclass).unwrap();

        // Add ivar to store window pointer for touch handling
        decl.add_ivar::<*mut std::ffi::c_void>(c"gpui_window_ptr");

        // Override layerClass to return CAMetalLayer
        extern "C" fn layer_class(_self: *const AnyClass, _sel: Sel) -> *const AnyClass {
            class!(CAMetalLayer) as *const AnyClass
        }

        // Touch handling methods
        extern "C" fn touches_began(
            this: *mut AnyObject,
            _sel: Sel,
            touches: *mut AnyObject,
            event: *mut AnyObject,
        ) {
            handle_touches(this, touches, event);
        }

        extern "C" fn touches_moved(
            this: *mut AnyObject,
            _sel: Sel,
            touches: *mut AnyObject,
            event: *mut AnyObject,
        ) {
            handle_touches(this, touches, event);
        }

        extern "C" fn touches_ended(
            this: *mut AnyObject,
            _sel: Sel,
            touches: *mut AnyObject,
            event: *mut AnyObject,
        ) {
            handle_touches(this, touches, event);
        }

        extern "C" fn touches_cancelled(
            this: *mut AnyObject,
            _sel: Sel,
            touches: *mut AnyObject,
            event: *mut AnyObject,
        ) {
            handle_touches(this, touches, event);
        }

        extern "C" fn hover_changed(this: *mut AnyObject, _sel: Sel, recognizer: *mut AnyObject) {
            handle_hover(this, recognizer);
        }

        extern "C" fn scroll_changed(this: *mut AnyObject, _sel: Sel, recognizer: *mut AnyObject) {
            handle_scroll(this, recognizer);
        }

        unsafe {
            // Add class method for layerClass
            decl.add_class_method(
                sel!(layerClass),
                layer_class as extern "C" fn(*const AnyClass, Sel) -> *const AnyClass,
            );

            // Add touch handling instance methods
            decl.add_method(
                sel!(touchesBegan:withEvent:),
                touches_began as extern "C" fn(*mut AnyObject, Sel, *mut AnyObject, *mut AnyObject),
            );
            decl.add_method(
                sel!(touchesMoved:withEvent:),
                touches_moved as extern "C" fn(*mut AnyObject, Sel, *mut AnyObject, *mut AnyObject),
            );
            decl.add_method(
                sel!(touchesEnded:withEvent:),
                touches_ended as extern "C" fn(*mut AnyObject, Sel, *mut AnyObject, *mut AnyObject),
            );
            decl.add_method(
                sel!(touchesCancelled:withEvent:),
                touches_cancelled
                    as extern "C" fn(*mut AnyObject, Sel, *mut AnyObject, *mut AnyObject),
            );
            decl.add_method(
                sel!(hoverChanged:),
                hover_changed as extern "C" fn(*mut AnyObject, Sel, *mut AnyObject),
            );
            decl.add_method(
                sel!(scrollChanged:),
                scroll_changed as extern "C" fn(*mut AnyObject, Sel, *mut AnyObject),
            );
        }

        decl.register();
    });

    class!(GPUIMetalView)
}

/// Register a custom UIView subclass that implements UIKeyInput protocol.
///
/// iOS requires the first-responder view to conform to `UIKeyInput` in order
/// for the software keyboard to actually route typed characters back to the
/// app.  Without this, `becomeFirstResponder` silently fails and no keyboard
/// appears.
///
/// The three required methods:
/// - `hasText` → always returns YES (simplifies things; no harm)
/// - `insertText:` → forwards the text to `IosWindow::handle_text_input`
/// - `deleteBackward` → dispatches a backspace via `crate::dispatch_text_input`
fn register_text_input_view_class() -> &'static AnyClass {
    TEXT_INPUT_VIEW_CLASS_REGISTERED.call_once(|| {
        let superclass = class!(UIView);
        let mut decl = ClassBuilder::new(c"GPUITextInputView", superclass).unwrap();

        // Declare protocol conformance so iOS knows this view can receive
        // keyboard text input.
        if let Some(protocol) = objc2::runtime::AnyProtocol::get(c"UIKeyInput") {
            decl.add_protocol(protocol);
        }

        // Store the IosWindow pointer so callbacks can reach the Rust window.
        decl.add_ivar::<*mut std::ffi::c_void>(c"gpui_window_ptr");

        // UITextInputTraits property storage — UIView doesn't provide these,
        // but iOS reads them from the first responder to configure the keyboard.
        decl.add_ivar::<isize>(c"_keyboardType"); // UIKeyboardType
        decl.add_ivar::<isize>(c"_autocorrectionType"); // UITextAutocorrectionType
        decl.add_ivar::<isize>(c"_autocapitalizationType"); // UITextAutocapitalizationType

        // --- UIKeyInput protocol methods ---

        // Bool hasText
        unsafe extern "C" fn has_text(_this: *mut AnyObject, _sel: Sel) -> Bool {
            Bool::YES
        }

        // void insertText:(NSString *)text
        unsafe extern "C" fn insert_text(this: *mut AnyObject, _sel: Sel, text: *mut AnyObject) {
            #[allow(deprecated)]
            let window_ptr: *mut std::ffi::c_void = *(*this).get_ivar(GPUI_WINDOW_IVAR);
            if window_ptr.is_null() || text.is_null() {
                return;
            }
            let window = &*(window_ptr as *const IosWindow);
            window.handle_text_input(text);
        }

        // void deleteBackward
        unsafe extern "C" fn delete_backward(this: *mut AnyObject, _sel: Sel) {
            #[allow(deprecated)]
            let window_ptr: *mut std::ffi::c_void = *(*this).get_ivar(GPUI_WINDOW_IVAR);
            if window_ptr.is_null() {
                return;
            }
            let window = &*(window_ptr as *const IosWindow);
            window.handle_delete_backward();
        }

        // canBecomeFirstResponder must return Bool::YES
        unsafe extern "C" fn can_become_first_responder(_this: *mut AnyObject, _sel: Sel) -> Bool {
            Bool::YES
        }

        // Handle physical keys here so they can repeat consistently while held.
        // The software keyboard still uses UIKeyInput.insertText.
        unsafe extern "C" fn presses_began(
            this: *mut AnyObject,
            _sel: Sel,
            presses: *mut AnyObject,
            event: *mut AnyObject,
        ) {
            if !handle_hardware_presses(this, presses, true) {
                let _: () =
                    msg_send![super(this, class!(UIView)), pressesBegan: presses, withEvent: event];
            }
        }

        unsafe extern "C" fn presses_ended(
            this: *mut AnyObject,
            _sel: Sel,
            presses: *mut AnyObject,
            event: *mut AnyObject,
        ) {
            if !handle_hardware_presses(this, presses, false) {
                let _: () =
                    msg_send![super(this, class!(UIView)), pressesEnded: presses, withEvent: event];
            }
        }

        unsafe extern "C" fn presses_cancelled(
            this: *mut AnyObject,
            _sel: Sel,
            presses: *mut AnyObject,
            event: *mut AnyObject,
        ) {
            if !handle_hardware_presses(this, presses, false) {
                let _: () =
                    msg_send![super(this, class!(UIView)), pressesCancelled: presses, withEvent: event];
            }
        }

        // --- UITextInputTraits property accessors ---
        #[allow(deprecated)]
        unsafe extern "C" fn get_keyboard_type(this: *mut AnyObject, _sel: Sel) -> isize {
            *(*this).get_ivar::<isize>("_keyboardType")
        }
        #[allow(deprecated)]
        unsafe extern "C" fn set_keyboard_type(this: *mut AnyObject, _sel: Sel, val: isize) {
            *(*this).get_mut_ivar::<isize>("_keyboardType") = val;
        }
        #[allow(deprecated)]
        unsafe extern "C" fn get_autocorrection_type(this: *mut AnyObject, _sel: Sel) -> isize {
            *(*this).get_ivar::<isize>("_autocorrectionType")
        }
        #[allow(deprecated)]
        unsafe extern "C" fn set_autocorrection_type(this: *mut AnyObject, _sel: Sel, val: isize) {
            *(*this).get_mut_ivar::<isize>("_autocorrectionType") = val;
        }
        #[allow(deprecated)]
        unsafe extern "C" fn get_autocapitalization_type(this: *mut AnyObject, _sel: Sel) -> isize {
            *(*this).get_ivar::<isize>("_autocapitalizationType")
        }
        #[allow(deprecated)]
        unsafe extern "C" fn set_autocapitalization_type(
            this: *mut AnyObject,
            _sel: Sel,
            val: isize,
        ) {
            *(*this).get_mut_ivar::<isize>("_autocapitalizationType") = val;
        }

        unsafe {
            decl.add_method(
                sel!(hasText),
                has_text as unsafe extern "C" fn(*mut AnyObject, Sel) -> Bool,
            );
            decl.add_method(
                sel!(insertText:),
                insert_text as unsafe extern "C" fn(*mut AnyObject, Sel, *mut AnyObject),
            );
            decl.add_method(
                sel!(deleteBackward),
                delete_backward as unsafe extern "C" fn(*mut AnyObject, Sel),
            );
            decl.add_method(
                sel!(canBecomeFirstResponder),
                can_become_first_responder as unsafe extern "C" fn(*mut AnyObject, Sel) -> Bool,
            );
            decl.add_method(
                sel!(pressesBegan:withEvent:),
                presses_began
                    as unsafe extern "C" fn(*mut AnyObject, Sel, *mut AnyObject, *mut AnyObject),
            );
            decl.add_method(
                sel!(pressesEnded:withEvent:),
                presses_ended
                    as unsafe extern "C" fn(*mut AnyObject, Sel, *mut AnyObject, *mut AnyObject),
            );
            decl.add_method(
                sel!(pressesCancelled:withEvent:),
                presses_cancelled
                    as unsafe extern "C" fn(*mut AnyObject, Sel, *mut AnyObject, *mut AnyObject),
            );

            // UITextInputTraits property methods
            decl.add_method(
                sel!(keyboardType),
                get_keyboard_type as unsafe extern "C" fn(*mut AnyObject, Sel) -> isize,
            );
            decl.add_method(
                sel!(setKeyboardType:),
                set_keyboard_type as unsafe extern "C" fn(*mut AnyObject, Sel, isize),
            );
            decl.add_method(
                sel!(autocorrectionType),
                get_autocorrection_type as unsafe extern "C" fn(*mut AnyObject, Sel) -> isize,
            );
            decl.add_method(
                sel!(setAutocorrectionType:),
                set_autocorrection_type as unsafe extern "C" fn(*mut AnyObject, Sel, isize),
            );
            decl.add_method(
                sel!(autocapitalizationType),
                get_autocapitalization_type as unsafe extern "C" fn(*mut AnyObject, Sel) -> isize,
            );
            decl.add_method(
                sel!(setAutocapitalizationType:),
                set_autocapitalization_type as unsafe extern "C" fn(*mut AnyObject, Sel, isize),
            );
        }

        decl.register();
    });

    class!(GPUITextInputView)
}

unsafe fn handle_hardware_presses(
    view: *mut AnyObject,
    presses: *mut AnyObject,
    down: bool,
) -> bool {
    #[allow(deprecated)]
    let window_ptr: *mut c_void = *(*view).get_ivar(GPUI_WINDOW_IVAR);
    if window_ptr.is_null() {
        return false;
    }
    let window = &*(window_ptr as *const IosWindow);
    let all: *mut AnyObject = msg_send![presses, allObjects];
    let count: usize = msg_send![all, count];
    let mut handled = false;
    for index in 0..count {
        let press: *mut AnyObject = msg_send![all, objectAtIndex: index];
        let key: *mut AnyObject = msg_send![press, key];
        if key.is_null() {
            continue;
        }
        let code: isize = msg_send![key, keyCode];
        let flags: usize = msg_send![key, modifierFlags];
        if !down {
            window.held_hardware_keys.borrow_mut().remove(&(code as u32));
        }
        let characters: *mut AnyObject = msg_send![key, characters];
        let characters = if characters.is_null() {
            None
        } else {
            let utf8: *const i8 = msg_send![characters, UTF8String];
            (!utf8.is_null()).then(|| {
                std::ffi::CStr::from_ptr(utf8)
                    .to_string_lossy()
                    .into_owned()
            })
        };
        let modified = flags & ((1 << 18) | (1 << 19) | (1 << 20)) != 0;
        let navigation = matches!(code, 0x29 | 0x49..=0x4B | 0x4D..=0x52);
        let action = if modified || navigation {
            Some(HardwareKeyAction::Key {
                code: code as u32,
                flags: flags as u32,
                characters,
            })
        } else if code == 0x2A {
            Some(HardwareKeyAction::Delete)
        } else {
            characters
                // Keep composed and non-ASCII input on UIKit's text path.
                .filter(|text| text.is_ascii() && text.chars().count() == 1)
                .map(HardwareKeyAction::Text)
        };
        if let Some(action) = action {
            if down {
                if code < 0xE0 && flags & (1 << 20) == 0 {
                    window.held_hardware_keys.borrow_mut().insert(
                        code as u32,
                        HeldHardwareKey {
                            action: action.clone(),
                            next_repeat: Instant::now() + KEY_REPEAT_DELAY,
                        },
                    );
                }
                window.dispatch_hardware_key(&action, false);
            } else {
                if let HardwareKeyAction::Key {
                    code,
                    flags,
                    characters,
                } = action
                {
                    window.handle_key_event_with_char(code, flags, false, characters.as_deref());
                }
            }
            handled = true;
        }
    }
    handled
}

/// Handle touch events from the GPUIMetalView
fn handle_touches(view: *mut AnyObject, touches: *mut AnyObject, event: *mut AnyObject) {
    unsafe {
        // Get the window pointer from the view's ivar
        #[allow(deprecated)]
        let window_ptr: *mut std::ffi::c_void = *(*view).get_ivar(GPUI_WINDOW_IVAR);
        if window_ptr.is_null() {
            log::warn!("GPUI iOS: Touch event but no window pointer set");
            return;
        }

        let window = &*(window_ptr as *const IosWindow);

        // Get all touches from the set
        let all_touches: *mut AnyObject = msg_send![touches, allObjects];
        let count: usize = msg_send![all_touches, count];

        for i in 0..count {
            let touch: *mut AnyObject = msg_send![all_touches, objectAtIndex: i];
            window.handle_touch(touch, event);
        }
    }
}

/// Forward unpressed trackpad and mouse movement to GPUI so hover styles work.
fn handle_hover(view: *mut AnyObject, recognizer: *mut AnyObject) {
    unsafe {
        #[allow(deprecated)]
        let window_ptr: *mut c_void = *(*view).get_ivar(GPUI_WINDOW_IVAR);
        if window_ptr.is_null() {
            return;
        }

        let window = &*(window_ptr as *const IosWindow);
        let state: isize = msg_send![recognizer, state];
        match state {
            // UIGestureRecognizerStateBegan / Changed
            1 | 2 => {
                let location: ObjcCGPoint = msg_send![recognizer, locationInView: view];
                let position = point(px(location.x as f32), px(location.y as f32));
                window.handle_hover(position, true);
            }
            // Ended / Cancelled / Failed
            3..=5 => window.handle_hover(point(px(-1.0), px(-1.0)), false),
            _ => {}
        }
    }
}

/// UIKit delivers trackpad and mouse-wheel scrolling to a pan recognizer,
/// rather than through UIView's touch callbacks.
fn handle_scroll(view: *mut AnyObject, recognizer: *mut AnyObject) {
    unsafe {
        #[allow(deprecated)]
        let window_ptr: *mut c_void = *(*view).get_ivar(GPUI_WINDOW_IVAR);
        if window_ptr.is_null() {
            return;
        }

        let state: isize = msg_send![recognizer, state];
        let phase = match state {
            1 => gpui::TouchPhase::Started,
            2 => gpui::TouchPhase::Moved,
            3..=5 => gpui::TouchPhase::Ended,
            _ => return,
        };
        let location: ObjcCGPoint = msg_send![recognizer, locationInView: view];
        let translation: ObjcCGPoint = msg_send![recognizer, translationInView: view];
        let _: () = msg_send![recognizer, setTranslation: ObjcCGPoint { x: 0.0, y: 0.0 }, inView: view];

        let window = &*(window_ptr as *const IosWindow);
        let position = point(px(location.x as f32), px(location.y as f32));
        window.mouse_position.set(position);
        if state == 1 {
            window.momentum_scroller.borrow_mut().cancel();
        }
        if let Some(callback) = window.input_callback.borrow_mut().as_mut() {
            callback(PlatformInput::ScrollWheel(gpui::ScrollWheelEvent {
                position,
                delta: gpui::ScrollDelta::Pixels(point(
                    px(translation.x as f32),
                    px(translation.y as f32),
                )),
                modifiers: window.modifiers.get(),
                touch_phase: phase,
            }));
        }
        if state == 3 {
            let velocity: ObjcCGPoint = msg_send![recognizer, velocityInView: view];
            window.momentum_scroller.borrow_mut().fling(
                velocity.x as f32,
                velocity.y as f32,
                location.x as f32,
                location.y as f32,
            );
        }
    }
}

/// iOS Window backed by UIWindow + UIViewController.
/// Distance (logical px) the finger must travel before a touch
/// is promoted from a potential tap to a scroll gesture.
const SCROLL_SLOP: f32 = 8.0;
/// A held touch that starts moving selects text instead of scrolling.
const SELECTION_HOLD_SECONDS: f64 = 0.4;

/// Tracks the current touch gesture state machine.
///
/// This distinguishes taps (short, stationary touches) from scroll gestures
/// (finger drags). The same pattern is used on Android.
#[derive(Clone, Copy, Debug)]
enum TouchState {
    /// No active touch.
    Idle,
    /// Finger is down but hasn't moved beyond the slop threshold.
    Pending {
        start_x: f32,
        start_y: f32,
        start_time: f64,
    },
    /// Finger has moved beyond the threshold — we are scrolling.
    Scrolling { prev_x: f32, prev_y: f32 },
    /// A long press promoted to a mouse drag for text selection.
    Selecting,
    /// A mouse or trackpad button press. Pointer clicks are sent on press.
    Pointer { button: gpui::MouseButton },
}

#[allow(clippy::type_complexity)]
pub(crate) struct IosWindow {
    /// The UIWindow object
    window: *mut AnyObject,
    /// The UIViewController
    view_controller: *mut AnyObject,
    /// The Metal-backed UIView
    view: *mut AnyObject,
    /// The hidden text input view for keyboard input
    text_input_view: *mut AnyObject,
    /// Current bounds in pixels
    bounds: Cell<Bounds<Pixels>>,
    /// Scale factor
    scale_factor: Cell<f32>,
    /// Input handler for text input
    input_handler: RefCell<Option<PlatformInputHandler>>,
    held_hardware_keys: RefCell<HashMap<u32, HeldHardwareKey>>,
    /// Callback for frame requests
    /// Note: pub(super) to allow ffi.rs to access this for the display link callback
    pub(super) request_frame_callback: RefCell<Option<Box<dyn FnMut(RequestFrameOptions)>>>,
    /// Callback for input events
    input_callback: RefCell<Option<Box<dyn FnMut(PlatformInput) -> DispatchEventResult>>>,
    /// Callback for active status changes
    active_status_callback: RefCell<Option<Box<dyn FnMut(bool)>>>,
    /// Callback for hover status changes from a trackpad or mouse
    hover_status_callback: RefCell<Option<Box<dyn FnMut(bool)>>>,
    hovered: Cell<bool>,
    /// Callback for resize events
    resize_callback: RefCell<Option<Box<dyn FnMut(Size<Pixels>, f32)>>>,
    /// Callback for move events (not applicable on iOS)
    moved_callback: RefCell<Option<Box<dyn FnMut()>>>,
    /// Callback for should close
    should_close_callback: RefCell<Option<Box<dyn FnMut() -> bool>>>,
    /// Callback for hit test
    hit_test_callback: RefCell<Option<Box<dyn FnMut() -> Option<WindowControlArea>>>>,
    /// Callback for close
    close_callback: RefCell<Option<Box<dyn FnOnce()>>>,
    /// Callback for appearance changes
    appearance_changed_callback: RefCell<Option<Box<dyn FnMut()>>>,
    /// Current mouse position (from touch)
    mouse_position: Cell<Point<Pixels>>,
    /// Current modifiers
    modifiers: Cell<Modifiers>,
    /// Track if a touch is currently pressed
    touch_pressed: Cell<bool>,
    /// Touch gesture state machine — distinguishes taps from scroll drags.
    touch_state: Cell<TouchState>,
    /// Velocity tracker — records recent touch samples during drag gestures
    /// so we can compute the release velocity when the finger lifts.
    velocity_tracker: RefCell<VelocityTracker>,
    /// Momentum scroller — produces decelerating scroll deltas after a fling
    /// gesture, driven by the CADisplayLink frame callback.
    momentum_scroller: RefCell<MomentumScroller>,
    /// The wgpu renderer (Metal backend on iOS).
    /// Wrapped in a `Mutex<Option<…>>` so that `draw()` (called from the
    /// `request_frame` callback) can acquire a mutable reference without
    /// conflicting with the outer `&self` borrow.
    renderer: Mutex<Option<WgpuRenderer>>,
}

// Required for raw_window_handle
unsafe impl Send for IosWindow {}
unsafe impl Sync for IosWindow {}

impl IosWindow {
    fn handle_hover(&self, position: Point<Pixels>, hovered: bool) {
        if self.hovered.replace(hovered) != hovered {
            if let Some(callback) = self.hover_status_callback.borrow_mut().as_mut() {
                callback(hovered);
            }
        }

        self.mouse_position.set(position);
        if !self.touch_pressed.get() {
            if let Some(callback) = self.input_callback.borrow_mut().as_mut() {
                callback(PlatformInput::MouseMove(gpui::MouseMoveEvent {
                    position,
                    modifiers: self.modifiers.get(),
                    pressed_button: None,
                }));
            }
        }
    }

    pub fn new(handle: AnyWindowHandle, _params: WindowParams) -> anyhow::Result<Self> {
        // Create the window on the main screen
        let screen = IosDisplay::main();
        let screen_bounds = screen.bounds();
        let scale_factor = screen.scale();

        unsafe {
            // Create UIWindow
            let screen_obj: *mut AnyObject = msg_send![class!(UIScreen), mainScreen];
            let screen_bounds_cg: ObjcCGRect = msg_send![screen_obj, bounds];
            let app: *mut AnyObject = msg_send![class!(UIApplication), sharedApplication];
            let scenes: *mut AnyObject = msg_send![app, connectedScenes];
            let scene: *mut AnyObject = msg_send![scenes, anyObject];
            let window: *mut AnyObject = msg_send![class!(UIWindow), alloc];
            let window: *mut AnyObject = msg_send![window, initWithWindowScene: scene];
            let _: () = msg_send![window, setFrame: screen_bounds_cg];

            // Create our custom UIViewController subclass that supports
            // dynamic `preferredStatusBarStyle` overrides.
            let vc_class = register_view_controller_class();
            let view_controller: *mut AnyObject = msg_send![vc_class, alloc];
            let view_controller: *mut AnyObject = msg_send![view_controller, init];
            let supports_trait_registration: Bool = msg_send![
                view_controller,
                respondsToSelector: sel!(registerForTraitChanges:withAction:)
            ];
            if supports_trait_registration.as_bool() {
                let trait_type: *mut AnyObject = class!(UITraitUserInterfaceStyle) as *const AnyClass as *mut AnyObject;
                let traits: *mut AnyObject = msg_send![class!(NSArray), arrayWithObject: trait_type];
                let _: *mut AnyObject = msg_send![
                    view_controller,
                    registerForTraitChanges: traits,
                    withAction: sel!(appearanceChanged)
                ];
            }
            log::info!("iZed appearance: trait registration supported={}", supports_trait_registration.as_bool());

            // Create our custom Metal view using the registered class
            let metal_view_class = register_metal_view_class();
            let view: *mut AnyObject = msg_send![metal_view_class, alloc];
            let view: *mut AnyObject = msg_send![view, initWithFrame: screen_bounds_cg];

            // Configure the Metal layer — wgpu will use it for rendering but
            // we still need to set contentsScale so the drawable size is correct.
            let layer: *mut AnyObject = msg_send![view, layer];
            let scale: core_graphics::base::CGFloat = msg_send![screen_obj, scale];
            let _: () = msg_send![layer, setContentsScale: scale];

            // Auto-resize the Metal view when the parent view changes size
            // (e.g. rotation). UIViewAutoresizingFlexibleWidth | UIViewAutoresizingFlexibleHeight
            let _: () = msg_send![view, setAutoresizingMask: 18_usize]; // 0x02 | 0x10

            // Enable user interaction on the Metal view for touch handling
            let _: () = msg_send![view, setUserInteractionEnabled: true];
            let _: () = msg_send![view, setMultipleTouchEnabled: true];

            let hover: *mut AnyObject = msg_send![class!(UIHoverGestureRecognizer), alloc];
            let hover: *mut AnyObject =
                msg_send![hover, initWithTarget: view, action: sel!(hoverChanged:)];
            let _: () = msg_send![view, addGestureRecognizer: hover];
            let _: () = msg_send![hover, release];

            // Recognize two-finger trackpad scrolling and mouse-wheel scrolling.
            // Exclude direct touches so the existing finger gesture state machine
            // continues to handle scrolling, selection, and taps.
            let scroll: *mut AnyObject = msg_send![class!(UIPanGestureRecognizer), alloc];
            let scroll: *mut AnyObject =
                msg_send![scroll, initWithTarget: view, action: sel!(scrollChanged:)];
            let _: () = msg_send![scroll, setAllowedScrollTypesMask: 3_isize];
            let no_touches: *mut AnyObject = msg_send![class!(NSArray), array];
            let _: () = msg_send![scroll, setAllowedTouchTypes: no_touches];
            let _: () = msg_send![view, addGestureRecognizer: scroll];
            let _: () = msg_send![scroll, release];

            // Set the view as the view controller's view
            let _: () = msg_send![view_controller, setView: view];

            // Set the root view controller
            let _: () = msg_send![window, setRootViewController: view_controller];

            // Make the window visible
            let _: () = msg_send![window, makeKeyAndVisible];

            // Create a hidden text input view for keyboard handling.
            // Uses our custom GPUITextInputView which implements UIKeyInput
            // so iOS actually routes keyboard text to us.
            let text_input_class = register_text_input_view_class();
            let text_input_view: *mut AnyObject = msg_send![text_input_class, alloc];
            let text_input_frame = ObjcCGRect::new(0.0, 0.0, 1.0, 1.0);
            let text_input_view: *mut AnyObject =
                msg_send![text_input_view, initWithFrame: text_input_frame];
            let _: () = msg_send![text_input_view, setAlpha: 0.01_f64];
            let _: () = msg_send![text_input_view, setUserInteractionEnabled: true];
            let _: () = msg_send![view, addSubview: text_input_view];

            // --- Initialise the wgpu renderer (Metal backend) ---------------
            let pixel_w = (screen_bounds_cg.width * scale) as i32;
            let pixel_h = (screen_bounds_cg.height * scale) as i32;

            let _handle = handle; // consumed but not stored
            let ios_window = Self {
                window,
                view_controller,
                view,
                text_input_view,
                bounds: Cell::new(screen_bounds),
                scale_factor: Cell::new(scale_factor),
                input_handler: RefCell::new(None),
                held_hardware_keys: RefCell::new(HashMap::new()),
                request_frame_callback: RefCell::new(None),
                input_callback: RefCell::new(None),
                active_status_callback: RefCell::new(None),
                hover_status_callback: RefCell::new(None),
                hovered: Cell::new(false),
                resize_callback: RefCell::new(None),
                moved_callback: RefCell::new(None),
                should_close_callback: RefCell::new(None),
                hit_test_callback: RefCell::new(None),
                close_callback: RefCell::new(None),
                appearance_changed_callback: RefCell::new(None),
                mouse_position: Cell::new(Point::default()),
                modifiers: Cell::new(Modifiers::default()),
                touch_pressed: Cell::new(false),
                touch_state: Cell::new(TouchState::Idle),
                velocity_tracker: RefCell::new(VelocityTracker::new()),
                momentum_scroller: RefCell::new(MomentumScroller::new()),
                renderer: Mutex::new(None),
            };

            // Create the wgpu renderer using the Metal backend.
            //
            // `gpui_wgpu::WgpuContext::instance()` only enables Vulkan+GL,
            // so we create our own wgpu instance with Metal enabled, build
            // a surface from the UIView's raw window handle, construct the
            // WgpuContext with that instance, and finally create the renderer.
            let config = WgpuSurfaceConfig {
                size: size(DevicePixels(pixel_w), DevicePixels(pixel_h)),
                transparent: false,
                preferred_present_mode: None,
            };

            let raw_window = RawIosWindow {
                view: ios_window.view as *mut c_void,
            };
            let metal_instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
                backends: wgpu::Backends::METAL,
                flags: wgpu::InstanceFlags::default(),
                backend_options: wgpu::BackendOptions::default(),
                memory_budget_thresholds: wgpu::MemoryBudgetThresholds::default(),
                display: Some(Box::new(raw_window)),
            });

            // Build a temporary surface for WgpuContext initialisation
            // (adapter selection needs a surface to test compatibility).
            let window_handle = raw_window
                .window_handle()
                .expect("iOS window handle unavailable");

            let target = wgpu::SurfaceTargetUnsafe::RawHandle {
                raw_display_handle: Some(raw_window.display_handle()?.as_raw()),
                raw_window_handle: window_handle.as_raw(),
            };

            let surface_result = metal_instance.create_surface_unsafe(target);
            match surface_result {
                Ok(surface) => match WgpuContext::new(metal_instance, &surface, None) {
                    Ok(context) => {
                        // Pre-populate gpu_context so WgpuRenderer::new()
                        // reuses our Metal-backed context (and its instance)
                        // instead of creating a Vulkan+GL one.
                        let gpu_context: GpuContext = Rc::new(RefCell::new(Some(context)));
                        drop(surface); // no longer needed — new() creates its own

                        match WgpuRenderer::new(gpu_context, &raw_window, config, None) {
                            Ok(renderer) => {
                                log::info!("iOS wgpu renderer created (Metal)");
                                *ios_window.renderer.lock() = Some(renderer);
                            }
                            Err(e) => {
                                log::error!("Failed to create iOS wgpu renderer: {e:#}");
                            }
                        }
                    }
                    Err(e) => {
                        log::error!("Failed to create iOS WgpuContext: {e:#}");
                    }
                },
                Err(e) => {
                    log::error!("Failed to create iOS wgpu Metal surface: {e:#}");
                }
            }

            Ok(ios_window)
        }
    }

    /// Get the raw pointer to the UIViewController.
    pub fn view_controller_ptr(&self) -> *mut AnyObject {
        self.view_controller
    }

    /// Get the raw pointer to the GPUIMetalView.
    pub fn metal_view_ptr(&self) -> *mut AnyObject {
        self.view
    }

    /// Register this window with the FFI layer after it's been stored.
    /// This must be called after the window is placed at a stable address
    /// (e.g., in a Box or Arc).
    pub(crate) fn register_with_ffi(&self) {
        super::ffi::register_window(self as *const Self);

        // Set the window pointer on the view so touch events can find us,
        // and on the text input view so keyboard input can find us.
        unsafe {
            let window_ptr = self as *const Self as *mut std::ffi::c_void;
            #[allow(deprecated)]
            {
                *(*self.view).get_mut_ivar::<*mut c_void>(GPUI_WINDOW_IVAR) = window_ptr;
            }
            #[allow(deprecated)]
            {
                *(*self.text_input_view).get_mut_ivar::<*mut c_void>(GPUI_WINDOW_IVAR) = window_ptr;
            }
            log::info!(
                "GPUI iOS: Set window pointer {:p} on view {:p} and text input {:p}",
                window_ptr,
                self.view,
                self.text_input_view
            );
        }

        // Listen for keyboard show/hide so we can expose the keyboard height.
        self.register_keyboard_observers();
    }

    /// Register for keyboard show/hide notifications so we can track the
    /// keyboard height and allow the UI to shift content above the keyboard.
    pub(crate) fn register_keyboard_observers(&self) {
        unsafe {
            let notification_center: *mut AnyObject =
                msg_send![class!(NSNotificationCenter), defaultCenter];

            let show_name = crate::ios::util::nsstring("UIKeyboardWillShowNotification");
            let hide_name = crate::ios::util::nsstring("UIKeyboardWillHideNotification");

            // Block that fires when the keyboard appears — extracts the
            // end-frame height and stores it in the global atomic.
            let show_block = block2::RcBlock::new(move |notification: *mut AnyObject| {
                if notification.is_null() {
                    return;
                }
                let user_info: *mut AnyObject = msg_send![notification, userInfo];
                if user_info.is_null() {
                    return;
                }
                let frame_key = crate::ios::util::nsstring("UIKeyboardFrameEndUserInfoKey");
                let frame_value: *mut AnyObject = msg_send![user_info, objectForKey: frame_key];
                // frame_key is autoreleased by util::nsstring — no manual release needed
                let _ = frame_key;
                if frame_value.is_null() {
                    return;
                }
                let frame: ObjcCGRect = msg_send![frame_value, CGRectValue];
                let height = frame.height as f32;
                log::info!("GPUI iOS: Keyboard will show, height={}", height);
                crate::set_keyboard_height(height);
            });

            let hide_block = block2::RcBlock::new(move |_notification: *mut AnyObject| {
                log::info!("GPUI iOS: Keyboard will hide");
                crate::set_keyboard_height(0.0);
            });

            let _: *mut AnyObject = msg_send![notification_center,
                addObserverForName: show_name,
                object: std::ptr::null::<AnyObject>(),
                queue: std::ptr::null::<AnyObject>(),
                usingBlock: &*show_block
            ];
            let _: *mut AnyObject = msg_send![notification_center,
                addObserverForName: hide_name,
                object: std::ptr::null::<AnyObject>(),
                queue: std::ptr::null::<AnyObject>(),
                usingBlock: &*hide_block
            ];
            // show_name and hide_name are autoreleased by util::nsstring

            // Leak the blocks so they live for the app lifetime.
            std::mem::forget(show_block);
            std::mem::forget(hide_block);
        }
    }

    /// Handle a touch event from UIKit.
    ///
    /// Uses a state machine to distinguish **taps** from **drag gestures**:
    ///
    ///   DOWN  → record start position, enter "pending" (NO MouseDown yet)
    ///   MOVE  → if finger moved > threshold → switch to "scrolling",
    ///           emit `ScrollWheel` deltas (for scrollable containers) AND
    ///           `MouseMove` (for interactive canvas screens like Animations)
    ///   UP    → if still "pending" → emit `MouseDown` + `MouseUp` (tap)
    ///           if "scrolling"   → emit final `ScrollWheel` (Ended) +
    ///           `MouseUp` (so drag-to-throw works)
    ///
    /// MouseDown is **deferred** until finger-up so that starting a scroll
    /// near a button or tab doesn't accidentally trigger navigation.
    /// Interactive screens use `MouseMove` to track the finger during drags
    /// and `MouseUp` to detect the end of a throw/drag gesture.
    pub fn handle_touch(&self, touch: *mut AnyObject, event: *mut AnyObject) {
        let position = touch_location_in_view(touch, self.view);
        let phase = touch_phase(touch);
        let tap_count = touch_tap_count(touch);
        let timestamp: f64 = unsafe { msg_send![touch, timestamp] };
        // UIKit sends mouse and trackpad clicks as indirect pointer touches.
        // UIEventButtonMaskSecondary is bit 1 (iOS 13.4+).
        let pointer_button = unsafe {
            let touch_type: isize = msg_send![touch, type];
            if touch_type == 3 {
                let button_mask: usize = if event.is_null() {
                    0
                } else {
                    msg_send![event, buttonMask]
                };
                Some(if button_mask & (1 << 1) != 0 {
                    gpui::MouseButton::Right
                } else {
                    gpui::MouseButton::Left
                })
            } else {
                None
            }
        };
        let modifiers = self.modifiers.get();

        let logical_x: f32 = position.x.into();
        let logical_y: f32 = position.y.into();

        self.mouse_position.set(position);

        let mut ts = self.touch_state.get();

        let emit = |input: PlatformInput| {
            if let Some(callback) = self.input_callback.borrow_mut().as_mut() {
                callback(input);
            }
        };

        match phase {
            UITouchPhase::Began => {
                self.touch_pressed.set(true);
                // Cancel any active momentum fling — the user touched the
                // screen again, so inertia scrolling must stop immediately.
                self.momentum_scroller.borrow_mut().cancel();
                self.velocity_tracker.borrow_mut().reset();

                if let Some(button) = pointer_button {
                    ts = TouchState::Pointer { button };
                    emit(PlatformInput::MouseDown(gpui::MouseDownEvent {
                        button,
                        position,
                        modifiers,
                        click_count: tap_count as usize,
                        first_mouse: false,
                    }));
                } else {
                    ts = TouchState::Pending {
                        start_x: logical_x,
                        start_y: logical_y,
                        start_time: timestamp,
                    };
                }
                // Do NOT emit MouseDown here — wait until we know whether
                // this is a tap or a scroll.  Emitting MouseDown immediately
                // causes accidental navigation when the user starts scrolling
                // near a button/tab.
                //
                // - Tap (finger lifts within slop) → emit MouseDown + MouseUp
                //   together in Ended phase.
                // - Scroll (finger exceeds slop) → emit only MouseMove +
                //   ScrollWheel, no MouseDown.
            }

            UITouchPhase::Moved => {
                // Record every move for velocity estimation.
                self.velocity_tracker
                    .borrow_mut()
                    .record(logical_x, logical_y);

                match ts {
                    TouchState::Pointer { button } => {
                        emit(PlatformInput::MouseMove(gpui::MouseMoveEvent {
                            position,
                            modifiers,
                            pressed_button: Some(button),
                        }));
                    }
                    TouchState::Pending {
                        start_x,
                        start_y,
                        start_time,
                    } => {
                        let dx = logical_x - start_x;
                        let dy = logical_y - start_y;
                        let distance = (dx * dx + dy * dy).sqrt();

                        if distance > SCROLL_SLOP {
                            if timestamp - start_time >= SELECTION_HOLD_SECONDS {
                                ts = TouchState::Selecting;
                                let start = gpui::point(gpui::px(start_x), gpui::px(start_y));
                                emit(PlatformInput::MouseDown(gpui::MouseDownEvent {
                                    button: gpui::MouseButton::Left,
                                    position: start,
                                    modifiers,
                                    click_count: 1,
                                    first_mouse: false,
                                }));
                            } else {
                                // A short drag scrolls the editor.
                                ts = TouchState::Scrolling {
                                    prev_x: logical_x,
                                    prev_y: logical_y,
                                };
                                emit(PlatformInput::ScrollWheel(gpui::ScrollWheelEvent {
                                    position,
                                    delta: gpui::ScrollDelta::Pixels(gpui::point(
                                        gpui::px(dx),
                                        gpui::px(dy),
                                    )),
                                    modifiers,
                                    touch_phase: gpui::TouchPhase::Started,
                                }));
                            }
                        }
                        // Always emit MouseMove so interactive screens can
                        // track finger position (e.g. drag line in Animations,
                        // gradient control in Shaders).
                        emit(PlatformInput::MouseMove(gpui::MouseMoveEvent {
                            position,
                            modifiers,
                            pressed_button: Some(gpui::MouseButton::Left),
                        }));
                    }
                    TouchState::Scrolling { prev_x, prev_y } => {
                        let dx = logical_x - prev_x;
                        let dy = logical_y - prev_y;
                        ts = TouchState::Scrolling {
                            prev_x: logical_x,
                            prev_y: logical_y,
                        };
                        // Scroll event for scrollable containers.
                        emit(PlatformInput::ScrollWheel(gpui::ScrollWheelEvent {
                            position,
                            delta: gpui::ScrollDelta::Pixels(gpui::point(
                                gpui::px(dx),
                                gpui::px(dy),
                            )),
                            modifiers,
                            touch_phase: gpui::TouchPhase::Moved,
                        }));
                        // MouseMove for interactive screens.
                        emit(PlatformInput::MouseMove(gpui::MouseMoveEvent {
                            position,
                            modifiers,
                            pressed_button: Some(gpui::MouseButton::Left),
                        }));
                    }
                    TouchState::Selecting => {
                        emit(PlatformInput::MouseMove(gpui::MouseMoveEvent {
                            position,
                            modifiers,
                            pressed_button: Some(gpui::MouseButton::Left),
                        }));
                    }
                    TouchState::Idle => {
                        // Spurious move without a preceding down — ignore.
                    }
                }
            }

            UITouchPhase::Ended | UITouchPhase::Cancelled => {
                self.touch_pressed.set(false);
                let was_pointer = matches!(ts, TouchState::Pointer { .. });
                match ts {
                    TouchState::Pointer { button } => {
                        emit(PlatformInput::MouseUp(gpui::MouseUpEvent {
                            button,
                            position,
                            modifiers,
                            click_count: tap_count as usize,
                        }));
                        if button == gpui::MouseButton::Left
                            && self.input_handler.borrow().is_some()
                        {
                            self.show_keyboard_with_type(crate::KeyboardType::Default);
                        }
                    }
                    TouchState::Pending {
                        start_x,
                        start_y,
                        start_time,
                    } => {
                        // Finger lifted without exceeding slop → tap.
                        // Emit MouseDown + MouseUp together at the original
                        // down position so hit-testing matches the initial
                        // touch point.
                        self.velocity_tracker.borrow_mut().reset();
                        let tap_pos = gpui::point(gpui::px(start_x), gpui::px(start_y));
                        let button = if timestamp - start_time >= SELECTION_HOLD_SECONDS {
                            gpui::MouseButton::Right
                        } else {
                            gpui::MouseButton::Left
                        };
                        emit(PlatformInput::MouseDown(gpui::MouseDownEvent {
                            button,
                            position: tap_pos,
                            modifiers,
                            click_count: tap_count as usize,
                            first_mouse: false,
                        }));
                        emit(PlatformInput::MouseUp(gpui::MouseUpEvent {
                            button,
                            position: tap_pos,
                            modifiers,
                            click_count: tap_count as usize,
                        }));
                        if button == gpui::MouseButton::Left
                            && self.input_handler.borrow().is_some()
                        {
                            self.show_keyboard_with_type(crate::KeyboardType::Default);
                        }
                    }
                    TouchState::Scrolling { prev_x, prev_y } => {
                        // End the active touch-scroll gesture.
                        let dx = logical_x - prev_x;
                        let dy = logical_y - prev_y;
                        emit(PlatformInput::ScrollWheel(gpui::ScrollWheelEvent {
                            position,
                            delta: gpui::ScrollDelta::Pixels(gpui::point(
                                gpui::px(dx),
                                gpui::px(dy),
                            )),
                            modifiers,
                            touch_phase: gpui::TouchPhase::Ended,
                        }));
                        // Also emit MouseUp so interactive screens can
                        // detect the end of a drag (e.g. fling a ball).
                        emit(PlatformInput::MouseUp(gpui::MouseUpEvent {
                            button: gpui::MouseButton::Left,
                            position,
                            modifiers,
                            click_count: 1,
                        }));

                        // ── Start momentum / inertia scrolling ───────────
                        // Compute release velocity from recent touch samples
                        // and kick off the momentum scroller.  Subsequent
                        // frames will pump synthetic ScrollWheel events via
                        // `pump_momentum()` until velocity decays below the
                        // threshold.
                        let (vx, vy) = self.velocity_tracker.borrow().velocity();
                        self.velocity_tracker.borrow_mut().reset();
                        self.momentum_scroller
                            .borrow_mut()
                            .fling(vx, vy, logical_x, logical_y);
                    }
                    TouchState::Selecting => {
                        self.velocity_tracker.borrow_mut().reset();
                        emit(PlatformInput::MouseUp(gpui::MouseUpEvent {
                            button: gpui::MouseButton::Left,
                            position,
                            modifiers,
                            click_count: 1,
                        }));
                    }
                    TouchState::Idle => {}
                }
                ts = TouchState::Idle;
                // A finger is not a hovering pointer. GPUI's hover hit test
                // otherwise keeps the last touch position after MouseUp,
                // leaving buttons visually highlighted after the finger lifts.
                if !was_pointer && !self.hovered.get() {
                    let outside = gpui::point(gpui::px(-1.0), gpui::px(-1.0));
                    self.mouse_position.set(outside);
                    emit(PlatformInput::MouseMove(gpui::MouseMoveEvent {
                        position: outside,
                        modifiers,
                        pressed_button: None,
                    }));
                }
            }

            UITouchPhase::Stationary => {
                // No change — ignore.
                return;
            }
        }

        self.touch_state.set(ts);
    }

    /// Query the safe area insets from the UIView.
    ///
    /// Returns `(top, bottom, left, right)` in logical points.
    /// These represent the areas occupied by system UI (status bar,
    /// home indicator, camera notch) that content should avoid.
    pub fn safe_area_insets(&self) -> (f32, f32, f32, f32) {
        if self.view.is_null() {
            return (0.0, 0.0, 0.0, 0.0);
        }
        unsafe {
            // UIEdgeInsets { top, left, bottom, right } — all CGFloat
            #[repr(C)]
            #[derive(Debug, Clone, Copy)]
            struct UIEdgeInsets {
                top: f64,
                left: f64,
                bottom: f64,
                right: f64,
            }

            unsafe impl Encode for UIEdgeInsets {
                const ENCODING: Encoding = Encoding::Struct(
                    "UIEdgeInsets",
                    &[
                        Encoding::Double,
                        Encoding::Double,
                        Encoding::Double,
                        Encoding::Double,
                    ],
                );
            }

            unsafe impl RefEncode for UIEdgeInsets {
                const ENCODING_REF: Encoding = Encoding::Pointer(&Self::ENCODING);
            }

            let insets: UIEdgeInsets = msg_send![self.view, safeAreaInsets];
            (
                insets.top as f32,
                insets.bottom as f32,
                insets.left as f32,
                insets.right as f32,
            )
        }
    }

    /// Advance the momentum scroller by one frame and emit a synthetic
    /// `ScrollWheel` event if the fling is still active.
    ///
    /// Called from `gpui_ios_request_frame` on every CADisplayLink tick,
    /// **before** the GPUI render callback runs, so that the scroll delta
    /// is picked up during the current frame's layout/paint cycle.
    pub(crate) fn pump_momentum(&self) {
        let mut scroller = self.momentum_scroller.borrow_mut();
        if !scroller.is_active() {
            return;
        }

        if let Some(delta) = scroller.step() {
            let modifiers = self.modifiers.get();
            let position = gpui::point(gpui::px(delta.position_x), gpui::px(delta.position_y));
            let fling_ended = !scroller.is_active();

            if let Some(callback) = self.input_callback.borrow_mut().as_mut() {
                callback(PlatformInput::ScrollWheel(gpui::ScrollWheelEvent {
                    position,
                    delta: gpui::ScrollDelta::Pixels(gpui::point(
                        gpui::px(delta.dx),
                        gpui::px(delta.dy),
                    )),
                    modifiers,
                    touch_phase: gpui::TouchPhase::Moved,
                }));

                // If this was the last momentum frame, send Ended now.
                if fling_ended {
                    callback(PlatformInput::ScrollWheel(gpui::ScrollWheelEvent {
                        position,
                        delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.0), gpui::px(0.0))),
                        modifiers,
                        touch_phase: gpui::TouchPhase::Ended,
                    }));
                }
            }
        } else {
            // Fling finished — emit one final Ended event so GPUI knows
            // the scroll gesture is truly complete.
            let position = gpui::point(
                gpui::px(scroller.position_x()),
                gpui::px(scroller.position_y()),
            );
            let modifiers = self.modifiers.get();
            if let Some(callback) = self.input_callback.borrow_mut().as_mut() {
                callback(PlatformInput::ScrollWheel(gpui::ScrollWheelEvent {
                    position,
                    delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.0), gpui::px(0.0))),
                    modifiers,
                    touch_phase: gpui::TouchPhase::Ended,
                }));
            }
        }
    }

    /// Repeat keys held on a physical keyboard. UIKit does not reliably send
    /// repeated UIPress events to our custom text input view.
    pub(crate) fn pump_key_repeats(&self) {
        let now = Instant::now();
        let due = {
            let mut held = self.held_hardware_keys.borrow_mut();
            held.values_mut()
                .filter_map(|key| {
                    if now >= key.next_repeat {
                        key.next_repeat = now + KEY_REPEAT_INTERVAL;
                        Some(key.action.clone())
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
        };
        for action in due {
            self.dispatch_hardware_key(&action, true);
        }
    }

    fn dispatch_hardware_key(&self, action: &HardwareKeyAction, repeated: bool) {
        match action {
            HardwareKeyAction::Text(text) => self.handle_input_text(text, repeated),
            HardwareKeyAction::Delete => self.handle_delete_backward(),
            HardwareKeyAction::Key {
                code,
                flags,
                characters,
            } => {
                self.handle_key_event_with_char(*code, *flags, true, characters.as_deref());
            }
        }
    }

    /// Show the software keyboard with the specified keyboard type.
    ///
    /// The actual `becomeFirstResponder` call is deferred to the next run-loop
    /// iteration via `performSelector:withObject:afterDelay:` to avoid re-entering
    /// GPUI's event dispatch while an entity lease is active (UIKit's keyboard
    /// presentation can synchronously trigger layout callbacks).
    pub fn show_keyboard_with_type(&self, keyboard_type: crate::KeyboardType) {
        log::info!("GPUI iOS: Showing keyboard (type={:?})", keyboard_type);
        unsafe {
            use crate::KeyboardType;
            let kb_type: isize = match keyboard_type {
                KeyboardType::Default => 0,      // UIKeyboardTypeDefault
                KeyboardType::EmailAddress => 7, // UIKeyboardTypeEmailAddress
                KeyboardType::Phone => 5,        // UIKeyboardTypePhonePad
                KeyboardType::NumberPad => 4,    // UIKeyboardTypeNumberPad
                KeyboardType::URL => 3,          // UIKeyboardTypeURL
                KeyboardType::Decimal => 8,      // UIKeyboardTypeDecimalPad
            };
            log::info!(
                "GPUI iOS: text_input_view={:p}, setKeyboardType: {}",
                self.text_input_view,
                kb_type
            );
            if self.text_input_view.is_null() {
                log::error!("GPUI iOS: text_input_view is NULL!");
                return;
            }
            let _: () = msg_send![self.text_input_view, setKeyboardType: kb_type];
            log::info!("GPUI iOS: setAutocorrectionType");
            let _: () = msg_send![self.text_input_view, setAutocorrectionType: 1_isize];
            log::info!("GPUI iOS: setAutocapitalizationType");
            let _: () = msg_send![self.text_input_view, setAutocapitalizationType: 0_isize];
            log::info!("GPUI iOS: scheduling becomeFirstResponder");

            // Defer becomeFirstResponder to the next run-loop iteration.
            let _: () = msg_send![self.text_input_view,
                performSelector: sel!(becomeFirstResponder),
                withObject: ptr::null::<AnyObject>(),
                afterDelay: 0.0_f64
            ];
            log::info!("GPUI iOS: show_keyboard_with_type done");
        }
    }

    /// Hide the software keyboard.
    ///
    /// Deferred to the next run-loop iteration (like `show_keyboard_with_type`)
    /// to avoid re-entering GPUI event dispatch.
    pub fn hide_keyboard(&self) {
        log::info!("GPUI iOS: Hiding keyboard");
        unsafe {
            let _: () = msg_send![self.text_input_view,
                performSelector: sel!(resignFirstResponder),
                withObject: ptr::null::<AnyObject>(),
                afterDelay: 0.0_f64
            ];
        }
    }

    /// Handle text input from the software keyboard
    pub fn handle_text_input(&self, text: *mut AnyObject) {
        if text.is_null() {
            return;
        }

        unsafe {
            // Convert NSString to Rust String
            let utf8: *const i8 = msg_send![text, UTF8String];
            if utf8.is_null() {
                return;
            }

            let text_str = std::ffi::CStr::from_ptr(utf8)
                .to_string_lossy()
                .into_owned();

            self.handle_input_text(&text_str, false);
        }
    }

    fn handle_input_text(&self, text_str: &str, repeated: bool) {
        log::info!("GPUI iOS: Text input: {:?}", text_str);

        // The global callback serves our TextInput components. Also route
        // keystrokes through GPUI so Vim and editor bindings can consume them.
        let dispatched = crate::dispatch_text_input(text_str);
        for c in text_str.chars() {
            let key = match c {
                '\n' | '\r' => "enter".to_string(),
                '\t' => "tab".to_string(),
                ' ' => "space".to_string(),
                '\u{1b}' => "escape".to_string(),
                c if c.is_ascii_uppercase() => c.to_ascii_lowercase().to_string(),
                c => c.to_string(),
            };
            let keystroke = gpui::Keystroke {
                modifiers: Modifiers {
                    shift: c.is_ascii_uppercase(),
                    ..Default::default()
                },
                key,
                key_char: Some(c.to_string()),
            };

            let event = PlatformInput::KeyDown(gpui::KeyDownEvent {
                keystroke,
                is_held: repeated,
                prefer_character_input: false,
            });

            let result = self
                .input_callback
                .borrow_mut()
                .as_mut()
                .map(|callback| callback(event));
            let handled = result.is_some_and(|result| !result.propagate || result.default_prevented);
            if !dispatched && !handled {
                if let Some(handler) = self.input_handler.borrow_mut().as_mut() {
                    handler.replace_text_in_range(None, &c.to_string());
                }
            }
        }
    }

    /// Handle the delete-backward action from the software keyboard.
    ///
    /// This is called by the `GPUITextInputView` when the user taps the
    /// backspace key.  We dispatch a special sentinel ("\x08") through the
    /// global text input callback so the active TextInput component can
    /// remove the last character.
    pub fn handle_delete_backward(&self) {
        log::info!("GPUI iOS: deleteBackward");

        // Try the global callback first (backspace = "\x08")
        crate::dispatch_text_input("\x08");

        // Always send a Backspace KeyDown event through GPUI to trigger
        // a re-render cycle (which runs drain_pending_text).
        let keystroke = gpui::Keystroke {
            modifiers: Modifiers::default(),
            key: "backspace".to_string(),
            key_char: None,
        };
        let event = PlatformInput::KeyDown(gpui::KeyDownEvent {
            keystroke,
            is_held: false,
            prefer_character_input: false,
        });
        if let Some(callback) = self.input_callback.borrow_mut().as_mut() {
            callback(event);
        }
    }

    /// Handle a key event from an external keyboard
    pub fn handle_key_event(&self, key_code: u32, modifier_flags: u32, is_key_down: bool) {
        self.handle_key_event_with_char(key_code, modifier_flags, is_key_down, None);
    }

    fn handle_key_event_with_char(
        &self,
        key_code: u32,
        modifier_flags: u32,
        is_key_down: bool,
        characters: Option<&str>,
    ) {
        use super::text_input::{
            key_code_to_key_down, key_code_to_key_up, key_code_to_string,
            modifier_flags_to_modifiers,
        };

        let key = key_code_to_string(key_code);
        let modifiers = modifier_flags_to_modifiers(modifier_flags);

        log::info!(
            "GPUI iOS: Key event - key: {:?}, modifiers: {:?}, down: {}",
            key,
            modifiers,
            is_key_down
        );

        // On key-down, dispatch cursor-movement control codes through the
        // global text input callback so TextField-based components receive them.
        if is_key_down {
            match key_code {
                0x50 => {
                    crate::dispatch_text_input("\x1b[D");
                } // Left arrow
                0x4F => {
                    crate::dispatch_text_input("\x1b[C");
                } // Right arrow
                0x4A => {
                    crate::dispatch_text_input("\x1b[H");
                } // Home
                0x4D => {
                    crate::dispatch_text_input("\x1b[F");
                } // End
                _ => {}
            }
        }

        let event = if is_key_down {
            key_code_to_key_down(key_code, modifier_flags, characters)
        } else {
            key_code_to_key_up(key_code, modifier_flags, characters)
        };

        if let Some(callback) = self.input_callback.borrow_mut().as_mut() {
            callback(event);
        }
    }

    /// Notify the window of active status changes (foreground/background).
    ///
    /// This is called by the FFI layer when the app transitions between
    /// foreground and background states.
    pub fn notify_active_status_change(&self, is_active: bool) {
        log::info!("GPUI iOS: Window active status changed to: {}", is_active);

        if !is_active {
            self.held_hardware_keys.borrow_mut().clear();
        }

        if let Some(callback) = self.active_status_callback.borrow_mut().as_mut() {
            callback(is_active);
        }
    }

    /// Handle a layout change (e.g. rotation, split-screen resize).
    ///
    /// Called from `viewDidLayoutSubviews` on the GPUIViewController.
    /// Queries the current UIView bounds, updates the stored bounds/scale,
    /// reconfigures the Metal layer + wgpu surface, and fires the resize callback.
    pub fn handle_layout_change(&self) {
        unsafe {
            let view_bounds: ObjcCGRect = msg_send![self.view, bounds];
            let screen: *mut AnyObject = msg_send![class!(UIScreen), mainScreen];
            let scale: core_graphics::base::CGFloat = msg_send![screen, scale];

            let new_w = view_bounds.width as f32;
            let new_h = view_bounds.height as f32;
            let new_scale = scale as f32;

            let old_bounds = self.bounds.get();
            let old_scale = self.scale_factor.get();

            let new_size = size(px(new_w), px(new_h));

            // Only process if something actually changed.
            if old_bounds.size == new_size && (old_scale - new_scale).abs() < 0.01 {
                return;
            }

            log::info!(
                "GPUI iOS: Layout changed — {:?} @{:.1}x → {:?} @{:.1}x",
                old_bounds.size,
                old_scale,
                new_size,
                new_scale,
            );

            // Update stored bounds (in logical pixels, matching GPUI convention).
            let new_bounds = Bounds {
                origin: Default::default(),
                size: new_size,
            };
            self.bounds.set(new_bounds);
            self.scale_factor.set(new_scale);

            // Update the Metal layer's contentsScale so the drawable has the
            // correct pixel dimensions.
            let layer: *mut AnyObject = msg_send![self.view, layer];
            let _: () = msg_send![layer, setContentsScale: scale];

            // Update the wgpu renderer's surface configuration.
            let pixel_w = (new_w * new_scale) as i32;
            let pixel_h = (new_h * new_scale) as i32;
            {
                let mut guard = self.renderer.lock();
                if let Some(renderer) = guard.as_mut() {
                    renderer
                        .update_drawable_size(size(DevicePixels(pixel_w), DevicePixels(pixel_h)));
                }
            }

            // Fire the resize callback so GPUI re-layouts at the new size.
            let cb = self.resize_callback.borrow_mut().take();
            if let Some(mut cb) = cb {
                cb(new_size, new_scale);
                // Restore the callback for future resize events.
                let mut slot = self.resize_callback.borrow_mut();
                if slot.is_none() {
                    *slot = Some(cb);
                }
            }
        }
    }
}

impl HasWindowHandle for IosWindow {
    fn window_handle(
        &self,
    ) -> std::result::Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError>
    {
        let view = NonNull::new(self.view as *mut c_void)
            .ok_or(raw_window_handle::HandleError::Unavailable)?;
        let handle = UiKitWindowHandle::new(view);
        Ok(unsafe { raw_window_handle::WindowHandle::borrow_raw(handle.into()) })
    }
}

impl HasDisplayHandle for IosWindow {
    fn display_handle(
        &self,
    ) -> std::result::Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError>
    {
        let handle = UiKitDisplayHandle::new();
        Ok(unsafe { raw_window_handle::DisplayHandle::borrow_raw(handle.into()) })
    }
}

impl PlatformWindow for IosWindow {
    fn bounds(&self) -> Bounds<Pixels> {
        self.bounds.get()
    }

    fn is_maximized(&self) -> bool {
        true // iOS windows are always "maximized"
    }

    fn window_bounds(&self) -> WindowBounds {
        WindowBounds::Fullscreen(self.bounds.get())
    }

    fn content_size(&self) -> Size<Pixels> {
        self.bounds.get().size
    }

    fn resize(&mut self, _size: Size<Pixels>) {
        // iOS windows cannot be resized programmatically
    }

    fn scale_factor(&self) -> f32 {
        self.scale_factor.get()
    }

    fn appearance(&self) -> WindowAppearance {
        unsafe {
            // The controller receives trait changes before its Metal view.
            // The appearance callback can run while the view still reports
            // the previous style, so use the controller that sent it.
            let trait_collection: *mut AnyObject = msg_send![self.view_controller, traitCollection];
            let style: i64 = msg_send![trait_collection, userInterfaceStyle];
            match style {
                2 => WindowAppearance::Dark,
                _ => WindowAppearance::Light,
            }
        }
    }

    fn display(&self) -> Option<Rc<dyn PlatformDisplay>> {
        Some(Rc::new(IosDisplay::main()))
    }

    fn mouse_position(&self) -> Point<Pixels> {
        self.mouse_position.get()
    }

    fn modifiers(&self) -> Modifiers {
        self.modifiers.get()
    }

    fn capslock(&self) -> Capslock {
        // Would need to check UIKeyModifierFlags
        Capslock { on: false }
    }

    fn set_input_handler(&mut self, input_handler: PlatformInputHandler) {
        *self.input_handler.borrow_mut() = Some(input_handler);
    }

    fn take_input_handler(&mut self) -> Option<PlatformInputHandler> {
        self.input_handler.borrow_mut().take()
    }

    fn prompt(
        &self,
        _level: PromptLevel,
        msg: &str,
        detail: Option<&str>,
        answers: &[PromptButton],
    ) -> Option<futures::channel::oneshot::Receiver<usize>> {
        let (tx, rx) = futures::channel::oneshot::channel();
        let tx = Arc::new(Mutex::new(Some(tx)));

        unsafe {
            // Create UIAlertController
            let title = std::ffi::CString::new(msg).ok()?;
            let message = std::ffi::CString::new(detail.unwrap_or("")).ok()?;

            let alert_style: i64 = 1; // UIAlertControllerStyleAlert

            let title_str: *mut AnyObject =
                msg_send![class!(NSString), stringWithUTF8String: title.as_ptr()];
            let message_str: *mut AnyObject =
                msg_send![class!(NSString), stringWithUTF8String: message.as_ptr()];

            let alert: *mut AnyObject = msg_send![
                class!(UIAlertController),
                alertControllerWithTitle: title_str,
                message: message_str,
                preferredStyle: alert_style
            ];

            // Add buttons
            for (index, button) in answers.iter().enumerate() {
                let label = std::ffi::CString::new(button.label().as_str()).ok()?;
                let button_title: *mut AnyObject = msg_send![
                    class!(NSString),
                    stringWithUTF8String: label.as_ptr()
                ];

                let action_style: i64 = if button.is_cancel() { 1 } else { 0 }; // UIAlertActionStyleCancel or Default

                let tx = tx.clone();
                let handler = block2::RcBlock::new(move |_action: *mut AnyObject| {
                    if let Some(tx) = tx.lock().take() {
                        let _ = tx.send(index);
                    }
                });
                let action: *mut AnyObject = msg_send![
                    class!(UIAlertAction),
                    actionWithTitle: button_title,
                    style: action_style,
                    handler: &*handler
                ];

                let _: () = msg_send![alert, addAction: action];
            }

            // Present the alert
            let _: () = msg_send![
                self.view_controller,
                presentViewController: alert,
                animated: true,
                completion: ptr::null::<AnyObject>()
            ];
        }

        Some(rx)
    }

    fn activate(&self) {
        unsafe {
            let _: () = msg_send![self.window, makeKeyAndVisible];
        }
    }

    fn is_active(&self) -> bool {
        unsafe {
            let app: *mut AnyObject = msg_send![class!(UIApplication), sharedApplication];
            let key_window: *mut AnyObject = msg_send![app, keyWindow];
            self.window == key_window
        }
    }

    fn is_hovered(&self) -> bool {
        self.hovered.get()
    }

    fn set_title(&mut self, _title: &str) {
        // iOS apps don't have window titles
    }

    fn background_appearance(&self) -> WindowBackgroundAppearance {
        WindowBackgroundAppearance::Opaque
    }

    fn set_background_appearance(&self, _background_appearance: WindowBackgroundAppearance) {
        // Could adjust view background color
    }

    fn minimize(&self) {
        // iOS apps cannot be minimized
    }

    fn zoom(&self) {
        // iOS apps cannot be zoomed
    }

    fn toggle_fullscreen(&self) {
        // iOS apps are always fullscreen
    }

    fn is_fullscreen(&self) -> bool {
        true
    }

    fn on_request_frame(&self, callback: Box<dyn FnMut(RequestFrameOptions)>) {
        *self.request_frame_callback.borrow_mut() = Some(callback);
    }

    fn on_input(&self, callback: Box<dyn FnMut(PlatformInput) -> DispatchEventResult>) {
        *self.input_callback.borrow_mut() = Some(callback);
    }

    fn on_active_status_change(&self, callback: Box<dyn FnMut(bool)>) {
        *self.active_status_callback.borrow_mut() = Some(callback);
    }

    fn on_hover_status_change(&self, callback: Box<dyn FnMut(bool)>) {
        *self.hover_status_callback.borrow_mut() = Some(callback);
    }

    fn on_resize(&self, callback: Box<dyn FnMut(Size<Pixels>, f32)>) {
        *self.resize_callback.borrow_mut() = Some(callback);
    }

    fn on_moved(&self, callback: Box<dyn FnMut()>) {
        *self.moved_callback.borrow_mut() = Some(callback);
    }

    fn on_should_close(&self, callback: Box<dyn FnMut() -> bool>) {
        *self.should_close_callback.borrow_mut() = Some(callback);
    }

    fn on_hit_test_window_control(&self, callback: Box<dyn FnMut() -> Option<WindowControlArea>>) {
        *self.hit_test_callback.borrow_mut() = Some(callback);
    }

    fn on_close(&self, callback: Box<dyn FnOnce()>) {
        *self.close_callback.borrow_mut() = Some(callback);
    }

    fn on_appearance_changed(&self, callback: Box<dyn FnMut()>) {
        *self.appearance_changed_callback.borrow_mut() = Some(callback);
    }

    fn draw(&self, scene: &Scene) {
        let mut guard = self.renderer.lock();
        if let Some(renderer) = guard.as_mut() {
            renderer.draw(scene);
        } else {
            log::trace!("GPUI iOS: draw called but no renderer available");
        }
    }

    fn sprite_atlas(&self) -> Arc<dyn PlatformAtlas> {
        let guard = self.renderer.lock();
        if let Some(renderer) = guard.as_ref() {
            renderer.sprite_atlas().clone()
        } else {
            // Fallback: return a dummy atlas so GPUI doesn't panic before
            // the renderer is initialised.
            Arc::new(FallbackAtlas::new())
        }
    }

    fn is_subpixel_rendering_supported(&self) -> bool {
        let guard = self.renderer.lock();
        guard
            .as_ref()
            .map(|r| r.supports_dual_source_blending())
            .unwrap_or(false)
    }

    fn gpu_specs(&self) -> Option<GpuSpecs> {
        let guard = self.renderer.lock();
        guard.as_ref().map(|r| r.gpu_specs())
    }

    fn update_ime_position(&self, _bounds: Bounds<Pixels>) {
        // iOS handles IME positioning automatically
    }
}

// ── Fallback atlas ────────────────────────────────────────────────────────────

/// A minimal fallback `PlatformAtlas` used until a real Blade/Metal renderer is
/// wired up.  It records tiles in memory but does not upload texture data to the
/// GPU — just enough to satisfy GPUI's atlas queries without panicking.
struct FallbackAtlas {
    state: Mutex<FallbackAtlasState>,
}

struct FallbackAtlasState {
    next_id: u32,
    tiles: HashMap<AtlasKey, AtlasTile>,
}

impl FallbackAtlas {
    fn new() -> Self {
        Self {
            state: Mutex::new(FallbackAtlasState {
                next_id: 1,
                tiles: HashMap::new(),
            }),
        }
    }
}

impl PlatformAtlas for FallbackAtlas {
    fn get_or_insert_with<'a>(
        &self,
        key: &AtlasKey,
        build: &mut dyn FnMut() -> anyhow::Result<
            Option<(Size<DevicePixels>, std::borrow::Cow<'a, [u8]>)>,
        >,
    ) -> anyhow::Result<Option<AtlasTile>> {
        let mut state = self.state.lock();

        if let Some(tile) = state.tiles.get(key) {
            return Ok(Some(tile.clone()));
        }

        let data = build()?;
        if let Some((size, _pixels)) = data {
            let id = state.next_id;
            state.next_id += 1;

            let tile = AtlasTile {
                texture_id: AtlasTextureId {
                    index: 0,
                    kind: AtlasTextureKind::Monochrome,
                },
                tile_id: TileId(id),
                padding: 0,
                bounds: Bounds {
                    origin: point(DevicePixels(0), DevicePixels(0)),
                    size,
                },
            };

            state.tiles.insert(key.clone(), tile.clone());
            Ok(Some(tile))
        } else {
            Ok(None)
        }
    }

    fn remove(&self, key: &AtlasKey) {
        self.state.lock().tiles.remove(key);
    }
}
