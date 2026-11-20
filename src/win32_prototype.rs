//! Developer-only Win32 / Direct2D canvas. Uses Studio's real project loader.
#![allow(unsafe_op_in_unsafe_fn)]
use crate::{
    project_io::{Completed, Job, Loaded},
    ui_benchmark::Session,
};
use eframe::egui;
use std::{cell::RefCell, path::PathBuf};
use windows::{
    Win32::{
        Foundation::*,
        Graphics::{
            Direct2D::{Common::*, *},
            Dxgi::Common::DXGI_FORMAT_R8G8B8A8_UNORM,
            Gdi::*,
        },
        System::LibraryLoader::GetModuleHandleW,
        UI::WindowsAndMessaging::*,
    },
    core::{Result, w},
};

struct Quad {
    bitmap: usize,
    corners: [[f32; 2]; 4],
    uv: [f32; 4],
}
struct State {
    target: ID2D1HwndRenderTarget,
    bitmaps: Vec<ID2D1Bitmap>,
    front: Vec<Quad>,
    back: Vec<Quad>,
    paths: Vec<Vec<[f64; 2]>>,
    page: [f32; 2],
    loaded: Option<Loaded>,
    job: Option<Job>,
    session: Option<Session>,
    back_side: bool,
    zoom: f32,
    pan: [f32; 2],
    drag: Option<([f32; 2], [f32; 2])>,
}
thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

fn rect(x: f32, y: f32, w: f32, h: f32) -> D2D_RECT_F {
    D2D_RECT_F {
        left: x,
        top: y,
        right: x + w,
        bottom: y + h,
    }
}
fn color(r: f32, g: f32, b: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r, g, b, a: 1. }
}

impl State {
    unsafe fn bitmap(&mut self, image: &egui::ColorImage) -> Result<usize> {
        let bytes: Vec<u8> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
        let bitmap = self.target.CreateBitmap(
            D2D_SIZE_U {
                width: image.size[0] as u32,
                height: image.size[1] as u32,
            },
            Some(bytes.as_ptr().cast()),
            image.size[0] as u32 * 4,
            &D2D1_BITMAP_PROPERTIES {
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_R8G8B8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                },
                dpiX: 96.,
                dpiY: 96.,
            },
        )?;
        let index = self.bitmaps.len();
        self.bitmaps.push(bitmap);
        Ok(index)
    }

    unsafe fn install(&mut self, loaded: Loaded) -> Result<()> {
        let layout = &loaded.project.settings.layout;
        self.page = [layout.page_width as f32, layout.page_height as f32];
        if let (Some(preview), Some(art)) = (&loaded.preview, &loaded.art) {
            let bitmap = self.bitmap(preview)?;
            let items = layout
                .items()
                .map_err(|e| windows::core::Error::new(E_FAIL, e))?;
            for (index, item) in items.into_iter().enumerate() {
                if layout.hidden_images.contains(&index) {
                    continue;
                }
                let uv = layout
                    .image_tiles
                    .get(index)
                    .map_or([0., 0., 1., 1.], |&[x, y, w, h]| {
                        [
                            x as f32 / art.pixels.width() as f32,
                            y as f32 / art.pixels.height() as f32,
                            w as f32 / art.pixels.width() as f32,
                            h as f32 / art.pixels.height() as f32,
                        ]
                    });
                self.front.push(Quad {
                    bitmap,
                    uv,
                    corners: item.orientation.unit_corners().map(|[x, y]| {
                        [
                            (item.x + x * item.width) as f32,
                            (item.y + y * item.height) as f32,
                        ]
                    }),
                });
            }
            self.paths = layout
                .page_cut_paths(art)
                .map_err(|e| windows::core::Error::new(E_FAIL, e))?;
        }
        let mut cache = std::collections::HashMap::new();
        for (item, (digest, pixels)) in loaded.project.back.images.iter().zip(&loaded.back_previews)
        {
            let bitmap = if let Some(index) = cache.get(digest) {
                *index
            } else {
                let index = self.bitmap(pixels)?;
                cache.insert(*digest, index);
                index
            };
            let mut uv = [0., 0., 1., 1.];
            if item.flip_x {
                uv.swap(0, 2);
            }
            if item.flip_y {
                uv.swap(1, 3);
            }
            self.back.push(Quad {
                bitmap,
                uv,
                corners: [[0., 0.], [1., 0.], [1., 1.], [0., 1.]].map(|p| {
                    let [x, y] = crate::duplex::BackEditor::benchmark_point(item, p);
                    [x as f32, y as f32]
                }),
            });
        }
        self.loaded = Some(loaded);
        Ok(())
    }

    unsafe fn paint(&mut self, hwnd: HWND) -> Result<()> {
        let mut paint = PAINTSTRUCT::default();
        BeginPaint(hwnd, &mut paint);
        let result = self.draw();
        let _ = EndPaint(hwnd, &paint);
        result
    }

    unsafe fn draw(&self) -> Result<()> {
        let size = self.target.GetSize();
        self.target.BeginDraw();
        self.target
            .SetTransform(&windows::Foundation::Numerics::Matrix3x2::identity());
        self.target.Clear(Some(&color(0.86, 0.86, 0.88)));
        let scale =
            ((size.width - 64.) / self.page[0]).min((size.height - 64.) / self.page[1]) * self.zoom;
        let x = (size.width - self.page[0] * scale) / 2. + self.pan[0];
        let y = (size.height - self.page[1] * scale) / 2. + self.pan[1];
        let brush = self
            .target
            .CreateSolidColorBrush(&color(1., 1., 1.), None)?;
        self.target.FillRectangle(
            &rect(x, y, self.page[0] * scale, self.page[1] * scale),
            &brush,
        );
        let items = if self.back_side {
            &self.back
        } else {
            &self.front
        };
        for quad in items {
            let bitmap = &self.bitmaps[quad.bitmap];
            let bitmap_size = bitmap.GetSize();
            let [u0, v0, u1, v1] = quad.uv;
            // Apply flips to the destination axes, keeping source bounds positive.
            let mut points = quad.corners;
            if u0 > u1 {
                points.swap(0, 1);
                points.swap(3, 2);
            }
            if v0 > v1 {
                points.swap(0, 3);
                points.swap(1, 2);
            }
            let origin = points[0];
            let right = points[1];
            let bottom = points[3];
            self.target
                .SetTransform(&windows::Foundation::Numerics::Matrix3x2 {
                    M11: (right[0] - origin[0]) * scale,
                    M12: (right[1] - origin[1]) * scale,
                    M21: (bottom[0] - origin[0]) * scale,
                    M22: (bottom[1] - origin[1]) * scale,
                    M31: x + origin[0] * scale,
                    M32: y + origin[1] * scale,
                });
            self.target.DrawBitmap(
                bitmap,
                Some(&rect(0., 0., 1., 1.)),
                1.,
                D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                Some(&rect(
                    u0.min(u1) * bitmap_size.width,
                    v0.min(v1) * bitmap_size.height,
                    (u1 - u0).abs() * bitmap_size.width,
                    (v1 - v0).abs() * bitmap_size.height,
                )),
            );
        }
        self.target
            .SetTransform(&windows::Foundation::Numerics::Matrix3x2::identity());
        brush.SetColor(&color(0.9, 0., 0.));
        for path in &self.paths {
            let transform = |p: [f64; 2]| {
                let p = if self.back_side {
                    let flip = self.loaded.as_ref().unwrap().project.back.flip;
                    flip.point(p, [self.page[0] as f64, self.page[1] as f64])
                } else {
                    p
                };
                D2D_POINT_2F {
                    x: x + p[0] as f32 * scale,
                    y: y + p[1] as f32 * scale,
                }
            };
            for edge in path.windows(2) {
                self.target
                    .DrawLine(transform(edge[0]), transform(edge[1]), &brush, 1., None);
            }
        }
        self.target.EndDraw(None, None)
    }
}

unsafe extern "system" fn procedure(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_DESTROY {
        PostQuitMessage(0);
        return LRESULT(0);
    }
    let handled = STATE.with(|state| {
        let Ok(mut state) = state.try_borrow_mut() else {
            return false;
        };
        let Some(state) = state.as_mut() else {
            return false;
        };
        let result: Result<()> = (|| {
            match message {
                WM_TIMER => {
                    if let Some(job) = &state.job {
                        match job.result.try_recv() {
                            Ok(Ok(Completed::Opened(loaded))) => {
                                state.job = None;
                                state.install(loaded)?;
                            }
                            Ok(_) | Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                                return Err(windows::core::Error::new(
                                    E_FAIL,
                                    "Project loading failed",
                                ));
                            }
                            Err(std::sync::mpsc::TryRecvError::Empty) => {}
                        }
                    }
                    let _ = InvalidateRect(hwnd, None, false);
                }
                WM_APP => {
                    if let Some(session) = &mut state.session {
                        if let Some((back, zoom, pan)) = session.step() {
                            state.back_side = back;
                            state.zoom = zoom;
                            state.pan = pan;
                        } else {
                            session.finish();
                            state.session = None;
                            DestroyWindow(hwnd)?;
                        }
                    }
                    let _ = InvalidateRect(hwnd, None, false);
                }
                WM_PAINT => {
                    state.paint(hwnd)?;
                    if state.loaded.is_some() {
                        let _ = KillTimer(hwnd, 1);
                        if state.session.is_some() {
                            let _ = PostMessageW(hwnd, WM_APP, WPARAM(0), LPARAM(0));
                        }
                    }
                }
                WM_SIZE => {
                    state.target.Resize(&D2D_SIZE_U {
                        width: (lparam.0 as u32 & 65535).max(1),
                        height: ((lparam.0 as u32 >> 16) & 65535).max(1),
                    })?;
                    let _ = InvalidateRect(hwnd, None, false);
                }
                WM_MOUSEWHEEL => {
                    state.zoom = (state.zoom
                        * ((wparam.0 >> 16) as u16 as i16 as f32 * 0.0015).exp())
                    .clamp(0.2, 16.);
                    let _ = InvalidateRect(hwnd, None, false);
                }
                WM_LBUTTONDOWN => {
                    let point = [
                        (lparam.0 as u16 as i16) as f32,
                        ((lparam.0 >> 16) as u16 as i16) as f32,
                    ];
                    state.drag = Some((point, state.pan));
                    windows::Win32::UI::Input::KeyboardAndMouse::SetCapture(hwnd);
                }
                WM_LBUTTONUP => {
                    state.drag = None;
                    let _ = windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture();
                }
                WM_MOUSEMOVE => {
                    if let Some((start, pan)) = state.drag {
                        state.pan = [
                            pan[0] + (lparam.0 as u16 as i16) as f32 - start[0],
                            pan[1] + ((lparam.0 >> 16) as u16 as i16) as f32 - start[1],
                        ];
                        let _ = InvalidateRect(hwnd, None, false);
                    }
                }
                WM_KEYDOWN if wparam.0 == 9 => {
                    state.back_side = !state.back_side;
                    let _ = InvalidateRect(hwnd, None, false);
                }
                WM_KEYDOWN if wparam.0 == 36 => {
                    state.zoom = 1.;
                    state.pan = [0.; 2];
                    let _ = InvalidateRect(hwnd, None, false);
                }
                _ => return Ok(()),
            }
            Ok(())
        })();
        if let Err(error) = result {
            let _ = std::fs::write("target/win32-prototype-error.txt", error.to_string());
            let _ = DestroyWindow(hwnd);
        }
        matches!(
            message,
            WM_TIMER
                | WM_APP
                | WM_PAINT
                | WM_SIZE
                | WM_MOUSEWHEEL
                | WM_LBUTTONDOWN
                | WM_LBUTTONUP
                | WM_MOUSEMOVE
                | WM_KEYDOWN
        )
    });
    if handled {
        LRESULT(0)
    } else {
        DefWindowProcW(hwnd, message, wparam, lparam)
    }
}

pub fn run(project: PathBuf, output: PathBuf) -> Result<()> {
    unsafe {
        let benchmark = output != std::path::Path::new("-");
        let session = Session::new("win32-direct2d-prototype", output);
        let instance = GetModuleHandleW(None)?;
        let class = w!("MC630Direct2DPrototype");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(procedure),
            hInstance: instance.into(),
            lpszClassName: class,
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            ..Default::default()
        };
        if RegisterClassW(&wc) == 0 {
            return Err(windows::core::Error::from_win32());
        }
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class,
            w!(
                "MC630 Win32 + Direct2D prototype — Tab: Main/Back · Wheel: zoom · Drag: pan · Home: fit"
            ),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            1360,
            900,
            None,
            None,
            instance,
            None,
        )?;
        let factory: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
        let mut client = RECT::default();
        GetClientRect(hwnd, &mut client)?;
        let target = factory.CreateHwndRenderTarget(
            &D2D1_RENDER_TARGET_PROPERTIES {
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_R8G8B8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_IGNORE,
                },
                dpiX: 96.,
                dpiY: 96.,
                ..Default::default()
            },
            &D2D1_HWND_RENDER_TARGET_PROPERTIES {
                hwnd,
                pixelSize: D2D_SIZE_U {
                    width: client.right as u32,
                    height: client.bottom as u32,
                },
                presentOptions: D2D1_PRESENT_OPTIONS_NONE,
            },
        )?;
        STATE.with(|state| {
            *state.borrow_mut() = Some(State {
                target,
                bitmaps: vec![],
                front: vec![],
                back: vec![],
                paths: vec![],
                page: [210., 297.],
                job: Some(Job::open(project)),
                loaded: None,
                session: benchmark.then_some(session),
                back_side: false,
                zoom: 1.,
                pan: [0.; 2],
                drag: None,
            })
        });
        let _ = ShowWindow(hwnd, SW_SHOW);
        SetTimer(hwnd, 1, 16, None);
        let mut message = MSG::default();
        while GetMessageW(&mut message, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        STATE.with(|state| *state.borrow_mut() = None);
        Ok(())
    }
}
