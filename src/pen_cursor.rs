//! OS-owned cursor position stays responsive independently of canvas rendering.
#[cfg(windows)]
mod native {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use std::cell::RefCell;
    use windows_sys::Win32::{
        Foundation::*,
        Graphics::Gdi::ScreenToClient,
        UI::{
            Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
            WindowsAndMessaging::*,
        },
    };
    const ID: usize = 0x6309;
    const REFRESH: u32 = WM_APP + 630;
    #[derive(Default)]
    struct State {
        hwnd: usize,
        cursors: [usize; 2],
        active: Option<([i32; 4], bool)>,
    }
    thread_local! { static STATE: RefCell<State> = RefCell::new(State::default()); }
    fn masks(closing: bool) -> ([u8; 128], [u8; 128]) {
        let mut and = [255; 128];
        let mut xor = [0; 128];
        let polygon = [(2., 2.), (5., 14.), (14., 18.), (18., 14.), (14., 5.)];
        let inside = |x: f32, y: f32| {
            let mut hit = false;
            for i in 0..5 {
                let (ax, ay) = polygon[i];
                let (bx, by) = polygon[(i + 1) % 5];
                if (ay > y) != (by > y) && x < (bx - ax) * (y - ay) / (by - ay) + ax {
                    hit = !hit;
                }
            }
            hit
        };
        for y in 0_usize..32 {
            for x in 0_usize..32 {
                let (fx, fy) = (x as f32, y as f32);
                let body = inside(fx, fy);
                let edge = body
                    && [(-1., 0.), (1., 0.), (0., -1.), (0., 1.)]
                        .iter()
                        .any(|(dx, dy)| !inside(fx + dx, fy + dy));
                let halo = !body
                    && (-1..=1).any(|dy| (-1..=1).any(|dx| inside(fx + dx as f32, fy + dy as f32)));
                let slit = body && x.abs_diff(y) <= 1 && x <= 10;
                let dot = closing && ((fx - 24.).hypot(fy - 6.) - 3.).abs() < 1.;
                if body || halo || dot {
                    let index = y * 4 + x / 8;
                    let bit = 128 >> (x % 8);
                    and[index] &= !bit;
                    if (body && !edge && !slit) || halo {
                        xor[index] |= bit;
                    }
                }
            }
        }
        (and, xor)
    }
    pub fn install(cc: &eframe::CreationContext<'_>) {
        let Ok(handle) = cc.window_handle() else {
            return;
        };
        let RawWindowHandle::Win32(handle) = handle.as_raw() else {
            return;
        };
        let hwnd = handle.hwnd.get() as HWND;
        STATE.with(|cell| {
            let mut state = cell.borrow_mut();
            for (i, closing) in [false, true].into_iter().enumerate() {
                let (and, xor) = masks(closing);
                state.cursors[i] = unsafe {
                    CreateCursor(
                        std::ptr::null_mut(),
                        2,
                        2,
                        32,
                        32,
                        and.as_ptr().cast(),
                        xor.as_ptr().cast(),
                    )
                } as usize;
            }
            if state.cursors.iter().all(|c| *c != 0)
                && unsafe { SetWindowSubclass(hwnd, Some(callback), ID, 0) } != 0
            {
                state.hwnd = hwnd as usize;
            } else {
                for cursor in &mut state.cursors {
                    if *cursor != 0 {
                        unsafe {
                            DestroyCursor(*cursor as HCURSOR);
                        };
                        *cursor = 0;
                    }
                }
            }
        });
    }
    pub fn begin_frame() {
        STATE.with(|s| s.borrow_mut().active = None);
    }
    fn set_cursor(hwnd: HWND) -> bool {
        STATE.with(|cell| {
            let state = cell.borrow();
            let Some((rect, closing)) = state.active else {
                return false;
            };
            let mut point = POINT { x: 0, y: 0 };
            if unsafe { GetCursorPos(&mut point) } == 0 || unsafe { WindowFromPoint(point) } != hwnd
            {
                return false;
            }
            if unsafe { ScreenToClient(hwnd, &mut point) } == 0 {
                return false;
            }
            if point.x < rect[0] || point.x > rect[2] || point.y < rect[1] || point.y > rect[3] {
                return false;
            }
            unsafe {
                SetCursor(state.cursors[usize::from(closing)] as HCURSOR);
            }
            true
        })
    }
    unsafe extern "system" fn callback(
        hwnd: HWND,
        msg: u32,
        w: WPARAM,
        l: LPARAM,
        _id: usize,
        _data: usize,
    ) -> LRESULT {
        if msg == WM_SETCURSOR && l as u16 == HTCLIENT as u16 && set_cursor(hwnd) {
            return 1;
        }
        if msg == REFRESH {
            set_cursor(hwnd);
            return 0;
        }
        if msg == WM_NCDESTROY {
            unsafe {
                RemoveWindowSubclass(hwnd, Some(callback), ID);
            }
            STATE.with(|cell| {
                let mut s = cell.borrow_mut();
                for c in s.cursors {
                    if c != 0 {
                        unsafe {
                            DestroyCursor(c as HCURSOR);
                        }
                    }
                }
                *s = State::default();
            });
        }
        unsafe { DefSubclassProc(hwnd, msg, w, l) }
    }
    pub fn show(ctx: &eframe::egui::Context, rect: eframe::egui::Rect, closing: bool) -> bool {
        STATE.with(|cell| {
            let mut s = cell.borrow_mut();
            if s.hwnd == 0 {
                return false;
            }
            let dpi = ctx.pixels_per_point();
            s.active = Some((
                [
                    (rect.left() * dpi) as i32,
                    (rect.top() * dpi) as i32,
                    (rect.right() * dpi) as i32,
                    (rect.bottom() * dpi) as i32,
                ],
                closing,
            ));
            // Process after eframe applies its standard cursor output.
            unsafe {
                PostMessageW(s.hwnd as HWND, REFRESH, 0, 0);
            }
            true
        })
    }
    #[cfg(test)]
    mod tests {
        #[test]
        fn nib_masks_are_visible_and_close_badge_differs() {
            let a = super::masks(false);
            let b = super::masks(true);
            assert!(a.0.iter().any(|v| *v != 255));
            assert!(a.1.iter().any(|v| *v != 0));
            assert_ne!(a, b);
        }
    }
}
#[cfg(windows)]
pub use native::{begin_frame, install, show};
#[cfg(not(windows))]
pub fn install(_: &eframe::CreationContext<'_>) {}
#[cfg(not(windows))]
pub fn begin_frame() {}
#[cfg(not(windows))]
pub fn show(_: &eframe::egui::Context, _: eframe::egui::Rect, _: bool) -> bool {
    false
}
