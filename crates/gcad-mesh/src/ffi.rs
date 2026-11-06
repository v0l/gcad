use crate::{Model, Options, build_with};
use std::ffi::{CStr, CString, c_char, c_int};

pub struct Handle {
    model: Model,
    error: CString,
}

fn handle(error: &str) -> *mut Handle {
    Box::into_raw(Box::new(Handle {
        model: Model::default(),
        error: CString::new(error.replace('\0', " ")).unwrap_or_default(),
    }))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn gcad_build(source: *const c_char, detail: f64, y_up: c_int) -> *mut Handle {
    if source.is_null() {
        return handle("null source");
    }
    let Ok(text) = unsafe { CStr::from_ptr(source) }.to_str() else {
        return handle("source is not UTF-8");
    };
    let options = Options {
        detail: if detail > 0.0 { detail } else { 1.0 },
        y_up: y_up != 0,
        vars: Vec::new(),
    };
    match std::panic::catch_unwind(|| build_with(text, &options)) {
        Ok(Ok(model)) => Box::into_raw(Box::new(Handle {
            model,
            error: CString::default(),
        })),
        Ok(Err(e)) => handle(&e.to_string()),
        Err(_) => handle("internal error"),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn gcad_free(h: *mut Handle) {
    if !h.is_null() {
        drop(unsafe { Box::from_raw(h) });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn gcad_error(h: *const Handle) -> *const c_char {
    match unsafe { h.as_ref() } {
        Some(h) if !h.error.as_bytes().is_empty() => h.error.as_ptr(),
        _ => std::ptr::null(),
    }
}

fn body<'a>(h: *const Handle, body: usize) -> Option<&'a crate::Body> {
    unsafe { h.as_ref() }?.model.bodies.get(body)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn gcad_body_count(h: *const Handle) -> usize {
    unsafe { h.as_ref() }.map_or(0, |h| h.model.bodies.len())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn gcad_vertex_count(h: *const Handle, b: usize) -> usize {
    body(h, b).map_or(0, |b| b.positions.len())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn gcad_index_count(h: *const Handle, b: usize) -> usize {
    body(h, b).map_or(0, |b| b.indices.len())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn gcad_joint_count(h: *const Handle, b: usize) -> usize {
    body(h, b).map_or(0, |b| b.joints.len())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn gcad_positions(h: *const Handle, b: usize) -> *const f32 {
    body(h, b).map_or(std::ptr::null(), |b| b.positions.as_ptr().cast())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn gcad_normals(h: *const Handle, b: usize) -> *const f32 {
    body(h, b).map_or(std::ptr::null(), |b| b.normals.as_ptr().cast())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn gcad_colours(h: *const Handle, b: usize) -> *const f32 {
    body(h, b).map_or(std::ptr::null(), |b| b.colours.as_ptr().cast())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn gcad_indices(h: *const Handle, b: usize) -> *const u32 {
    body(h, b).map_or(std::ptr::null(), |b| b.indices.as_ptr())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn gcad_joint_indices(h: *const Handle, b: usize) -> *const u16 {
    body(h, b).map_or(std::ptr::null(), |b| b.joint_indices.as_ptr().cast())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn gcad_joint_weights(h: *const Handle, b: usize) -> *const f32 {
    body(h, b).map_or(std::ptr::null(), |b| b.joint_weights.as_ptr().cast())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn gcad_clip_count(h: *const Handle, b: usize) -> usize {
    body(h, b).map_or(0, |b| b.clips.len())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn gcad_clip_duration(h: *const Handle, b: usize, clip: usize) -> f32 {
    body(h, b)
        .and_then(|b| b.clips.get(clip))
        .map_or(0.0, |c| c.duration)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn gcad_bone_matrices(
    h: *const Handle,
    b: usize,
    clip: c_int,
    t: f64,
    out: *mut f32,
    capacity: usize,
) -> usize {
    let Some(body) = body(h, b) else { return 0 };
    let clip = usize::try_from(clip).ok();
    let matrices = body.bone_matrices(clip, t);
    if out.is_null() || capacity < matrices.len() * 16 {
        return matrices.len();
    }
    for (i, m) in matrices.iter().enumerate() {
        unsafe { std::ptr::copy_nonoverlapping(m.as_ptr(), out.add(i * 16), 16) };
    }
    matrices.len()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn gcad_glb(h: *const Handle, len: *mut usize) -> *mut u8 {
    let Some(h) = (unsafe { h.as_ref() }) else {
        return std::ptr::null_mut();
    };
    match h.model.to_glb() {
        Ok(bytes) => {
            let boxed = bytes.into_boxed_slice();
            if !len.is_null() {
                unsafe { *len = boxed.len() };
            }
            Box::into_raw(boxed).cast::<u8>()
        }
        Err(_) => std::ptr::null_mut(),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn gcad_glb_free(data: *mut u8, len: usize) {
    if !data.is_null() {
        drop(unsafe { Box::from_raw(std::ptr::slice_from_raw_parts_mut(data, len)) });
    }
}
