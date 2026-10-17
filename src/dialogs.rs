//! Native desktop dialogs and file open picker.
use std::path::PathBuf;

/// Open a native desktop Open File Dialog to select media or subtitle files.
#[cfg(windows)]
pub fn open_file_dialog() -> Option<PathBuf> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;

    let lib = unsafe { libloading::Library::new("comdlg32.dll").ok()? };

    #[repr(C)]
    struct OpenFileNameW {
        l_struct_size: u32,
        hwnd_owner: *mut std::ffi::c_void,
        h_instance: *mut std::ffi::c_void,
        lpstr_filter: *const u16,
        lpstr_custom_filter: *mut u16,
        n_max_cust_filter: u32,
        n_filter_index: u32,
        lpstr_file: *mut u16,
        n_max_file: u32,
        lpstr_file_title: *mut u16,
        n_max_file_title: u32,
        lpstr_initial_dir: *const u16,
        lpstr_title: *const u16,
        flags: u32,
        n_file_offset: u16,
        n_file_extension: u16,
        lpstr_def_ext: *const u16,
        l_cust_data: usize,
        lpfn_hook: *mut std::ffi::c_void,
        lp_template_name: *const u16,
        pv_reserved: *mut std::ffi::c_void,
        dw_reserved: u32,
        flags_ex: u32,
    }

    type GetOpenFileNameWFn = unsafe extern "system" fn(*mut OpenFileNameW) -> i32;
    let get_open_file_name: libloading::Symbol<GetOpenFileNameWFn> = unsafe { lib.get(b"GetOpenFileNameW\0").ok()? };

    let mut file_buf = vec![0u16; 2048];
    // Filter string: pairs of null-terminated strings ending with double null
    let filter: Vec<u16> = "Media Files (*.mp4, *.mkv, *.avi, *.mp3...)\0*.mp4;*.mkv;*.avi;*.mov;*.webm;*.flac;*.mp3;*.wav;*.m4v;*.wmv;*.ts;*.vob;*.ogg;*.aac;*.opus;*.srt;*.vtt;*.ass\0All Files (*.*)\0*.*\0\0"
        .encode_utf16().collect();
    let title: Vec<u16> = "Select Media - TermVLC\0".encode_utf16().collect();

    let mut ofn: OpenFileNameW = unsafe { std::mem::zeroed() };
    ofn.l_struct_size = std::mem::size_of::<OpenFileNameW>() as u32;
    ofn.lpstr_filter = filter.as_ptr();
    ofn.lpstr_file = file_buf.as_mut_ptr();
    ofn.n_max_file = file_buf.len() as u32;
    ofn.lpstr_title = title.as_ptr();
    ofn.flags = 0x00000800 | 0x00001000; // OFN_PATHMUSTEXIST | OFN_FILEMUSTEXIST

    let res = unsafe { get_open_file_name(&mut ofn) };
    if res != 0 {
        let len = file_buf.iter().position(|&c| c == 0).unwrap_or(file_buf.len());
        let os_str = OsString::from_wide(&file_buf[..len]);
        Some(PathBuf::from(os_str))
    } else {
        None
    }
}

#[cfg(not(windows))]
pub fn open_file_dialog() -> Option<PathBuf> {
    None
}
