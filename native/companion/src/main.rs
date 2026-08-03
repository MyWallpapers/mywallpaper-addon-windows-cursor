use std::ffi::c_void;
use std::io::{self, Read, Write};
use std::mem::size_of;
use std::ptr;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use windows::core::BOOL;
use windows::Win32::Graphics::Gdi::{
    CreateBitmap, CreateDIBSection, DeleteObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
    DIB_RGB_COLORS, HGDIOBJ,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateIconIndirect, DestroyIcon, SetSystemCursor, SystemParametersInfoW, HCURSOR, ICONINFO,
    OCR_NORMAL, SPI_SETCURSORS, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
};

const PROTOCOL_VERSION: u32 = 3;
const MAX_FRAME_BYTES: usize = 1024 * 1024;

#[derive(Deserialize)]
#[serde(tag = "type")]
enum HostFrame {
    #[serde(rename = "init")]
    Init {
        v: u32,
        #[serde(rename = "layerSettings")]
        _layer_settings: Value,
        #[serde(rename = "deviceSettings")]
        device_settings: Value,
    },
    #[serde(rename = "settings")]
    Settings {
        v: u32,
        #[serde(rename = "layerSettings")]
        _layer_settings: Value,
        #[serde(rename = "deviceSettings")]
        device_settings: Value,
    },
    #[serde(rename = "message")]
    Message {
        v: u32,
        #[serde(rename = "surface")]
        _surface: RendererSurface,
        #[serde(rename = "payload")]
        _payload: Value,
    },
    #[serde(rename = "shutdown")]
    Shutdown { v: u32 },
}

#[derive(Serialize)]
#[serde(tag = "type")]
enum CompanionFrame<T: Serialize> {
    #[serde(rename = "ready")]
    Ready { v: u32 },
    #[serde(rename = "message")]
    Message {
        v: u32,
        target: MessageTarget,
        payload: T,
    },
    #[serde(rename = "error")]
    Error {
        v: u32,
        message: String,
        code: String,
    },
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
enum RendererSurface {
    Interface,
    Wallpaper,
}

#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
enum MessageTarget {
    Broadcast,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CursorStatus {
    kind: &'static str,
    active: bool,
    message: String,
}

struct CursorRestorer {
    applied: bool,
}

impl CursorRestorer {
    fn restore(&mut self) -> Result<(), String> {
        restore_system_cursors()?;
        self.applied = false;
        Ok(())
    }
}

impl Drop for CursorRestorer {
    fn drop(&mut self) {
        if self.applied {
            let _ = restore_system_cursors();
        }
    }
}

#[derive(Clone, Copy)]
struct Color {
    red: u8,
    green: u8,
    blue: u8,
}

struct CursorSettings {
    enabled: bool,
    size: u32,
    fill: Color,
    outline: Color,
    accent: Color,
    outline_width: u32,
}

fn main() -> Result<(), String> {
    if std::env::var("MYWALLPAPER_PROTOCOL").as_deref() != Ok("process-v2") {
        return Err("MYWALLPAPER_PROTOCOL must be process-v2".to_owned());
    }
    let output = Arc::new(Mutex::new(io::stdout()));
    let mut restorer = CursorRestorer { applied: false };
    let mut input = io::stdin();
    let mut initialized = false;

    while let Some(frame) =
        read_frame::<HostFrame>(&mut input).map_err(|error| error.to_string())?
    {
        let version = match &frame {
            HostFrame::Init { v, .. }
            | HostFrame::Settings { v, .. }
            | HostFrame::Message { v, .. }
            | HostFrame::Shutdown { v } => *v,
        };
        if version != PROTOCOL_VERSION {
            write_error(
                &output,
                "protocol-version",
                format!("unsupported protocol version {version}"),
            )?;
            break;
        }
        match frame {
            HostFrame::Init {
                device_settings, ..
            } if !initialized => {
                let status = apply_settings(&device_settings, &mut restorer)?;
                write_frame(
                    &output,
                    &CompanionFrame::<Value>::Ready {
                        v: PROTOCOL_VERSION,
                    },
                )?;
                send_status(&output, status)?;
                initialized = true;
            }
            HostFrame::Settings {
                device_settings, ..
            } if initialized => {
                send_status(&output, apply_settings(&device_settings, &mut restorer)?)?;
            }
            HostFrame::Message { .. } if initialized => {}
            HostFrame::Shutdown { .. } => break,
            _ => {
                write_error(
                    &output,
                    "protocol-state",
                    "invalid companion lifecycle frame".to_owned(),
                )?;
                break;
            }
        }
    }
    restorer.restore()?;
    Ok(())
}

fn apply_settings(value: &Value, restorer: &mut CursorRestorer) -> Result<CursorStatus, String> {
    let settings = parse_settings(value)?;
    restorer.restore()?;
    if !settings.enabled {
        return Ok(CursorStatus {
            kind: "cursor.status",
            active: false,
            message: "Using the Windows cursor scheme".to_owned(),
        });
    }
    let cursor = create_cursor(&settings)?;
    unsafe {
        if let Err(error) = SetSystemCursor(cursor, OCR_NORMAL) {
            let _ = DestroyIcon(windows::Win32::UI::WindowsAndMessaging::HICON(cursor.0));
            return Err(format!("SetSystemCursor failed: {error}"));
        }
    }
    restorer.applied = true;
    Ok(CursorStatus {
        kind: "cursor.status",
        active: true,
        message: "Custom normal pointer is active".to_owned(),
    })
}

fn parse_settings(value: &Value) -> Result<CursorSettings, String> {
    let enabled = value
        .get("enabled")
        .and_then(Value::as_bool)
        .ok_or("enabled must be a boolean")?;
    let size = match value.get("size").and_then(Value::as_str) {
        Some("24") => 24,
        Some("32") => 32,
        Some("40") => 40,
        Some("48") => 48,
        _ => return Err("size must be one of 24, 32, 40, or 48".to_owned()),
    };
    let outline_width = value
        .get("outlineWidth")
        .and_then(Value::as_u64)
        .filter(|width| (1..=4).contains(width))
        .ok_or("outlineWidth must be between 1 and 4")? as u32;
    Ok(CursorSettings {
        enabled,
        size,
        fill: parse_color(value.get("fillColor"), "fillColor")?,
        outline: parse_color(value.get("outlineColor"), "outlineColor")?,
        accent: parse_color(value.get("glowColor"), "glowColor")?,
        outline_width,
    })
}

fn parse_color(value: Option<&Value>, label: &str) -> Result<Color, String> {
    let text = value
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{label} must be a color"))?;
    if text.len() != 7 || !text.starts_with('#') {
        return Err(format!("{label} must use #RRGGBB"));
    }
    let component = |range: std::ops::Range<usize>| {
        u8::from_str_radix(&text[range], 16).map_err(|_| format!("{label} must use #RRGGBB"))
    };
    Ok(Color {
        red: component(1..3)?,
        green: component(3..5)?,
        blue: component(5..7)?,
    })
}

fn create_cursor(settings: &CursorSettings) -> Result<HCURSOR, String> {
    let size = settings.size as usize;
    let mut pixels = vec![0_u32; size * size];
    let polygon = scaled_polygon(size as f32);
    for y in 0..size {
        for x in 0..size {
            let index = y * size + x;
            if !inside_polygon(x as f32 + 0.5, y as f32 + 0.5, &polygon) {
                continue;
            }
            let distance = edge_distance(x as f32 + 0.5, y as f32 + 0.5, &polygon);
            let color = if distance < settings.outline_width as f32 {
                settings.outline
            } else if distance < settings.outline_width as f32 + 1.2 {
                settings.accent
            } else {
                settings.fill
            };
            pixels[index] = 0xff00_0000
                | ((color.red as u32) << 16)
                | ((color.green as u32) << 8)
                | color.blue as u32;
        }
    }
    unsafe {
        let mut bits = ptr::null_mut::<c_void>();
        let bitmap_info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: settings.size as i32,
                biHeight: -(settings.size as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                biSizeImage: (pixels.len() * size_of::<u32>()) as u32,
                ..Default::default()
            },
            ..Default::default()
        };
        let color_bitmap = CreateDIBSection(None, &bitmap_info, DIB_RGB_COLORS, &mut bits, None, 0)
            .map_err(|error| format!("cursor color bitmap failed: {error}"))?;
        if bits.is_null() {
            let _ = DeleteObject(HGDIOBJ(color_bitmap.0));
            return Err("cursor color bitmap has no writable pixels".to_owned());
        }
        ptr::copy_nonoverlapping(
            pixels.as_ptr().cast::<u8>(),
            bits.cast::<u8>(),
            pixels.len() * size_of::<u32>(),
        );
        let mask_bitmap = CreateBitmap(settings.size as i32, settings.size as i32, 1, 1, None);
        if mask_bitmap.0.is_null() {
            let _ = DeleteObject(HGDIOBJ(color_bitmap.0));
            return Err("cursor mask bitmap failed".to_owned());
        }
        let info = ICONINFO {
            fIcon: BOOL(0),
            xHotspot: 2,
            yHotspot: 2,
            hbmMask: mask_bitmap,
            hbmColor: color_bitmap,
        };
        let icon =
            CreateIconIndirect(&info).map_err(|error| format!("cursor creation failed: {error}"));
        let _ = DeleteObject(HGDIOBJ(mask_bitmap.0));
        let _ = DeleteObject(HGDIOBJ(color_bitmap.0));
        icon.map(|value| HCURSOR(value.0))
    }
}

fn scaled_polygon(size: f32) -> Vec<(f32, f32)> {
    [
        (0.08, 0.05),
        (0.08, 0.82),
        (0.34, 0.61),
        (0.52, 0.96),
        (0.72, 0.87),
        (0.55, 0.55),
        (0.89, 0.55),
    ]
    .into_iter()
    .map(|(x, y)| (x * size, y * size))
    .collect()
}

fn inside_polygon(x: f32, y: f32, polygon: &[(f32, f32)]) -> bool {
    let mut inside = false;
    let mut previous = polygon.len() - 1;
    for current in 0..polygon.len() {
        let (xi, yi) = polygon[current];
        let (xj, yj) = polygon[previous];
        if (yi > y) != (yj > y)
            && x < (xj - xi) * (y - yi) / ((yj - yi).abs().max(f32::EPSILON)) + xi
        {
            inside = !inside;
        }
        previous = current;
    }
    inside
}

fn edge_distance(x: f32, y: f32, polygon: &[(f32, f32)]) -> f32 {
    let mut distance = f32::MAX;
    for index in 0..polygon.len() {
        let (ax, ay) = polygon[index];
        let (bx, by) = polygon[(index + 1) % polygon.len()];
        let dx = bx - ax;
        let dy = by - ay;
        let length = dx * dx + dy * dy;
        let t = if length == 0.0 {
            0.0
        } else {
            ((x - ax) * dx + (y - ay) * dy) / length
        }
        .clamp(0.0, 1.0);
        distance = distance.min(((x - (ax + t * dx)).powi(2) + (y - (ay + t * dy)).powi(2)).sqrt());
    }
    distance
}

fn restore_system_cursors() -> Result<(), String> {
    unsafe {
        SystemParametersInfoW(
            SPI_SETCURSORS,
            0,
            None,
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    }
    .map_err(|error| format!("restoring the Windows cursor scheme failed: {error}"))
}

fn send_status(output: &Arc<Mutex<io::Stdout>>, payload: CursorStatus) -> Result<(), String> {
    write_frame(
        output,
        &CompanionFrame::Message {
            v: PROTOCOL_VERSION,
            target: MessageTarget::Broadcast,
            payload,
        },
    )
}

fn read_frame<T: for<'de> Deserialize<'de>>(reader: &mut impl Read) -> io::Result<Option<T>> {
    let mut prefix = [0_u8; 4];
    match reader.read(&mut prefix[..1])? {
        0 => return Ok(None),
        1 => reader.read_exact(&mut prefix[1..])?,
        _ => unreachable!(),
    }
    let length = u32::from_le_bytes(prefix) as usize;
    if length == 0 || length > MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid frame length",
        ));
    }
    let mut payload = vec![0; length];
    reader.read_exact(&mut payload)?;
    serde_json::from_slice(&payload)
        .map(Some)
        .map_err(io::Error::other)
}

fn write_frame<T: Serialize>(output: &Arc<Mutex<io::Stdout>>, value: &T) -> Result<(), String> {
    let payload = serde_json::to_vec(value).map_err(|error| error.to_string())?;
    if payload.is_empty() || payload.len() > MAX_FRAME_BYTES {
        return Err("outbound frame has an invalid size".to_owned());
    }
    let mut output = output
        .lock()
        .map_err(|_| "output lock poisoned".to_owned())?;
    output
        .write_all(&(payload.len() as u32).to_le_bytes())
        .and_then(|()| output.write_all(&payload))
        .and_then(|()| output.flush())
        .map_err(|error| error.to_string())
}

fn write_error(output: &Arc<Mutex<io::Stdout>>, code: &str, message: String) -> Result<(), String> {
    write_frame(
        output,
        &CompanionFrame::<Value>::Error {
            v: PROTOCOL_VERSION,
            message,
            code: code.to_owned(),
        },
    )
}
