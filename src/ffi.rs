//! C ABI used by the Python wrapper.

use std::cell::RefCell;
use std::ffi::{c_char, CStr, CString};
use std::fmt::Write;
use std::ptr;
use std::slice;

use crate::{DocumentMeta, IndexOptions, SearchEngine, SearchOptions, SearchResult, TermStat};

thread_local! {
    static LAST_ERROR: RefCell<Option<CString>> = RefCell::new(None);
}

fn clear_last_error() {
    LAST_ERROR.with(|slot| {
        *slot.borrow_mut() = None;
    });
}

fn set_last_error(message: impl Into<String>) {
    let message = message.into().replace('\0', "\\0");
    let message = CString::new(message)
        .unwrap_or_else(|_| CString::new("failed to construct error message").unwrap());

    LAST_ERROR.with(|slot| {
        *slot.borrow_mut() = Some(message);
    });
}

fn string_into_raw(value: String) -> *mut c_char {
    match CString::new(value) {
        Ok(value) => value.into_raw(),
        Err(_) => {
            set_last_error("string contained an interior null byte");
            ptr::null_mut()
        }
    }
}

fn json_escape(value: &str, output: &mut String) {
    output.push('"');
    for ch in value.chars() {
        match ch {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\u{08}' => output.push_str("\\b"),
            '\u{0C}' => output.push_str("\\f"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            ch if ch <= '\u{1F}' => {
                let _ = write!(output, "\\u{:04X}", ch as u32);
            }
            _ => output.push(ch),
        }
    }
    output.push('"');
}

fn push_json_string_field(output: &mut String, key: &str, value: &str) {
    json_escape(key, output);
    output.push(':');
    json_escape(value, output);
}

fn push_json_u64_field(output: &mut String, key: &str, value: u64) {
    json_escape(key, output);
    output.push(':');
    let _ = write!(output, "{value}");
}

fn push_json_f64_field(output: &mut String, key: &str, value: f64) {
    json_escape(key, output);
    output.push(':');
    let _ = write!(output, "{value}");
}

fn push_json_optional_string_field(output: &mut String, key: &str, value: Option<&str>) {
    json_escape(key, output);
    output.push(':');
    match value {
        Some(value) => json_escape(value, output),
        None => output.push_str("null"),
    }
}

fn push_json_optional_u64_field(output: &mut String, key: &str, value: Option<u64>) {
    json_escape(key, output);
    output.push(':');
    match value {
        Some(value) => {
            let _ = write!(output, "{value}");
        }
        None => output.push_str("null"),
    }
}

fn push_json_string_array(output: &mut String, values: &[String]) {
    output.push('[');
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            output.push(',');
        }
        json_escape(value, output);
    }
    output.push(']');
}

fn serialize_search_results(results: &[SearchResult]) -> String {
    let mut output = String::from("[");
    for (index, result) in results.iter().enumerate() {
        if index > 0 {
            output.push(',');
        }

        output.push('{');
        push_json_u64_field(&mut output, "doc_id", result.doc_id as u64);
        output.push(',');
        push_json_string_field(&mut output, "path", &result.path);
        output.push(',');
        push_json_f64_field(&mut output, "score", result.score);
        output.push(',');
        json_escape("matched_terms", &mut output);
        output.push(':');
        push_json_string_array(&mut output, &result.matched_terms);
        output.push(',');
        push_json_optional_string_field(&mut output, "snippet", result.snippet.as_deref());
        output.push('}');
    }
    output.push(']');
    output
}

fn serialize_term_stats(stats: &[TermStat]) -> String {
    let mut output = String::from("[");
    for (index, stat) in stats.iter().enumerate() {
        if index > 0 {
            output.push(',');
        }

        output.push('{');
        push_json_string_field(&mut output, "term", &stat.term);
        output.push(',');
        push_json_u64_field(
            &mut output,
            "document_frequency",
            stat.document_frequency as u64,
        );
        output.push(',');
        push_json_u64_field(&mut output, "total_frequency", stat.total_frequency as u64);
        output.push('}');
    }
    output.push(']');
    output
}

fn serialize_document(document: &DocumentMeta) -> String {
    let mut output = String::from("{");
    push_json_u64_field(&mut output, "id", document.id as u64);
    output.push(',');
    push_json_string_field(&mut output, "path", &document.path);
    output.push(',');
    push_json_u64_field(&mut output, "length", document.length as u64);
    output.push(',');
    push_json_string_field(&mut output, "content", &document.content);
    output.push(',');
    push_json_optional_string_field(&mut output, "extension", document.extension.as_deref());
    output.push(',');
    push_json_string_field(&mut output, "title", &document.title);
    output.push(',');
    push_json_optional_u64_field(
        &mut output,
        "modified_unix_timestamp_secs",
        document.modified_unix_timestamp_secs,
    );
    output.push('}');
    output
}

fn serialize_documents(documents: &[DocumentMeta]) -> String {
    let mut output = String::from("[");
    for (index, document) in documents.iter().enumerate() {
        if index > 0 {
            output.push(',');
        }
        output.push_str(&serialize_document(document));
    }
    output.push(']');
    output
}

fn serialize_vocabulary(vocabulary: &[String]) -> String {
    let mut output = String::new();
    push_json_string_array(&mut output, vocabulary);
    output
}

fn usize_from_u64(value: u64, label: &str) -> Result<usize, String> {
    usize::try_from(value).map_err(|_| format!("{label} is too large"))
}

unsafe fn engine_ref<'a>(engine: *const SearchEngine) -> Result<&'a SearchEngine, String> {
    if engine.is_null() {
        return Err("engine handle must not be null".to_string());
    }

    // SAFETY: The caller promises `engine` points to a valid `SearchEngine`.
    unsafe { engine.as_ref() }.ok_or_else(|| "engine handle must not be null".to_string())
}

unsafe fn engine_mut<'a>(engine: *mut SearchEngine) -> Result<&'a mut SearchEngine, String> {
    if engine.is_null() {
        return Err("engine handle must not be null".to_string());
    }

    // SAFETY: The caller promises `engine` points to a valid `SearchEngine`.
    unsafe { engine.as_mut() }.ok_or_else(|| "engine handle must not be null".to_string())
}

unsafe fn read_required_string(ptr: *const c_char, label: &str) -> Result<String, String> {
    if ptr.is_null() {
        return Err(format!("{label} must not be null"));
    }

    // SAFETY: The caller promises `ptr` points to a valid null-terminated string.
    let value = unsafe { CStr::from_ptr(ptr) };
    value
        .to_str()
        .map(str::to_string)
        .map_err(|_| format!("{label} must be valid UTF-8"))
}

unsafe fn read_optional_string(ptr: *const c_char, label: &str) -> Result<Option<String>, String> {
    if ptr.is_null() {
        return Ok(None);
    }

    // SAFETY: `ptr` is non-null and is expected to point to a valid string.
    unsafe { read_required_string(ptr, label) }.map(Some)
}

unsafe fn build_index_options(
    has_extensions: bool,
    extensions: *const *const c_char,
    extensions_len: u64,
    has_max_file_size_bytes: bool,
    max_file_size_bytes: u64,
) -> Result<IndexOptions, String> {
    let mut options = IndexOptions::new();

    if has_extensions {
        let len = usize_from_u64(extensions_len, "extensions_len")?;
        if len > 0 && extensions.is_null() {
            return Err("extensions pointer must not be null when extensions_len > 0".to_string());
        }

        let extension_values = if len == 0 {
            Vec::new()
        } else {
            // SAFETY: The caller provides `len` valid pointers in `extensions`.
            let slice = unsafe { slice::from_raw_parts(extensions, len) };
            let mut values = Vec::with_capacity(slice.len());
            for &extension in slice {
                // SAFETY: Each extension pointer is expected to point to a valid string.
                values.push(unsafe { read_required_string(extension, "extension") }?);
            }
            values
        };

        options = options.with_extensions(extension_values);
    }

    if has_max_file_size_bytes {
        options = options.with_max_file_size_bytes(max_file_size_bytes);
    }

    Ok(options)
}

#[no_mangle]
pub extern "C" fn minisearch_engine_new() -> *mut SearchEngine {
    clear_last_error();
    Box::into_raw(Box::new(SearchEngine::new()))
}

#[no_mangle]
pub unsafe extern "C" fn minisearch_engine_free(engine: *mut SearchEngine) {
    if engine.is_null() {
        return;
    }

    // SAFETY: `engine` was allocated by `Box::into_raw` in this module.
    drop(unsafe { Box::from_raw(engine) });
}

#[no_mangle]
pub extern "C" fn minisearch_last_error_message() -> *mut c_char {
    LAST_ERROR.with(|slot| {
        let borrowed = slot.borrow();
        match borrowed.as_ref() {
            Some(message) => match CString::new(message.as_bytes()) {
                Ok(message) => message.into_raw(),
                Err(_) => ptr::null_mut(),
            },
            None => ptr::null_mut(),
        }
    })
}

#[no_mangle]
pub unsafe extern "C" fn minisearch_string_free(value: *mut c_char) {
    if value.is_null() {
        return;
    }

    // SAFETY: `value` was allocated by `CString::into_raw` in this module.
    drop(unsafe { CString::from_raw(value) });
}

#[no_mangle]
pub unsafe extern "C" fn minisearch_engine_add_document(
    engine: *mut SearchEngine,
    path: *const c_char,
    content: *const c_char,
) -> bool {
    clear_last_error();

    let result = (|| -> Result<(), String> {
        // SAFETY: The raw engine handle comes from this module.
        let engine = unsafe { engine_mut(engine) }?;
        // SAFETY: C callers must pass valid UTF-8 strings.
        let path = unsafe { read_required_string(path, "path") }?;
        // SAFETY: C callers must pass valid UTF-8 strings.
        let content = unsafe { read_required_string(content, "content") }?;
        engine.add_document(path, &content);
        Ok(())
    })();

    match result {
        Ok(()) => true,
        Err(error) => {
            set_last_error(error);
            false
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn minisearch_engine_build_from_directory(
    directory: *const c_char,
    has_extensions: bool,
    extensions: *const *const c_char,
    extensions_len: u64,
    has_max_file_size_bytes: bool,
    max_file_size_bytes: u64,
) -> *mut SearchEngine {
    clear_last_error();

    let result = (|| {
        // SAFETY: C callers must pass valid UTF-8 strings.
        let directory = unsafe { read_required_string(directory, "directory") }?;
        // SAFETY: The caller must provide a valid array of UTF-8 strings when enabled.
        let options = unsafe {
            build_index_options(
                has_extensions,
                extensions,
                extensions_len,
                has_max_file_size_bytes,
                max_file_size_bytes,
            )
        }?;
        SearchEngine::build_from_directory_with_options(&directory, &options)
            .map(Box::new)
            .map(Box::into_raw)
            .map_err(|error| error.to_string())
    })();

    match result {
        Ok(engine) => engine,
        Err(error) => {
            set_last_error(error);
            ptr::null_mut()
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn minisearch_engine_index_directory(
    engine: *mut SearchEngine,
    directory: *const c_char,
    has_extensions: bool,
    extensions: *const *const c_char,
    extensions_len: u64,
    has_max_file_size_bytes: bool,
    max_file_size_bytes: u64,
) -> i64 {
    clear_last_error();

    let result = (|| {
        // SAFETY: The raw engine handle comes from this module.
        let engine = unsafe { engine_mut(engine) }?;
        // SAFETY: C callers must pass valid UTF-8 strings.
        let directory = unsafe { read_required_string(directory, "directory") }?;
        // SAFETY: The caller must provide a valid array of UTF-8 strings when enabled.
        let options = unsafe {
            build_index_options(
                has_extensions,
                extensions,
                extensions_len,
                has_max_file_size_bytes,
                max_file_size_bytes,
            )
        }?;
        let count = engine
            .index_directory_with_options(&directory, &options)
            .map_err(|error| error.to_string())?;
        i64::try_from(count).map_err(|_| "indexed document count exceeds i64 range".to_string())
    })();

    match result {
        Ok(count) => count,
        Err(error) => {
            set_last_error(error);
            -1
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn minisearch_engine_search(
    engine: *const SearchEngine,
    query: *const c_char,
    top_k: u64,
    path_prefix: *const c_char,
    has_min_score: bool,
    min_score: f64,
) -> *mut c_char {
    clear_last_error();

    let result = (|| {
        // SAFETY: The raw engine handle comes from this module.
        let engine = unsafe { engine_ref(engine) }?;
        // SAFETY: C callers must pass valid UTF-8 strings.
        let query = unsafe { read_required_string(query, "query") }?;
        // SAFETY: Null means no filter, otherwise a valid UTF-8 string is required.
        let path_prefix = unsafe { read_optional_string(path_prefix, "path_prefix") }?;

        let mut options = SearchOptions::new(usize_from_u64(top_k, "top_k")?);
        if let Some(path_prefix) = path_prefix {
            options = options.with_path_prefix(path_prefix);
        }
        if has_min_score {
            options = options.with_min_score(min_score);
        }

        let results = engine.search_with_options(&query, &options);
        let json = serialize_search_results(&results);
        let raw = string_into_raw(json);
        if raw.is_null() {
            return Err("failed to allocate search results string".to_string());
        }
        Ok(raw)
    })();

    match result {
        Ok(value) => value,
        Err(error) => {
            set_last_error(error);
            ptr::null_mut()
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn minisearch_engine_save_to_path(
    engine: *const SearchEngine,
    path: *const c_char,
) -> bool {
    clear_last_error();

    let result = (|| {
        // SAFETY: The raw engine handle comes from this module.
        let engine = unsafe { engine_ref(engine) }?;
        // SAFETY: C callers must pass valid UTF-8 strings.
        let path = unsafe { read_required_string(path, "path") }?;
        engine
            .save_to_path(&path)
            .map_err(|error| error.to_string())
    })();

    match result {
        Ok(()) => true,
        Err(error) => {
            set_last_error(error);
            false
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn minisearch_engine_load_from_path(
    path: *const c_char,
) -> *mut SearchEngine {
    clear_last_error();

    let result = (|| {
        // SAFETY: C callers must pass valid UTF-8 strings.
        let path = unsafe { read_required_string(path, "path") }?;
        SearchEngine::load_from_path(&path)
            .map(Box::new)
            .map(Box::into_raw)
            .map_err(|error| error.to_string())
    })();

    match result {
        Ok(engine) => engine,
        Err(error) => {
            set_last_error(error);
            ptr::null_mut()
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn minisearch_engine_document_count(engine: *const SearchEngine) -> u64 {
    clear_last_error();

    match unsafe { engine_ref(engine) } {
        Ok(engine) => engine.document_count() as u64,
        Err(error) => {
            set_last_error(error);
            0
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn minisearch_engine_vocabulary_size(engine: *const SearchEngine) -> u64 {
    clear_last_error();

    match unsafe { engine_ref(engine) } {
        Ok(engine) => engine.vocabulary_size() as u64,
        Err(error) => {
            set_last_error(error);
            0
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn minisearch_engine_average_document_length(
    engine: *const SearchEngine,
) -> f64 {
    clear_last_error();

    match unsafe { engine_ref(engine) } {
        Ok(engine) => engine.average_document_length(),
        Err(error) => {
            set_last_error(error);
            0.0
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn minisearch_engine_vocabulary(engine: *const SearchEngine) -> *mut c_char {
    clear_last_error();

    let result = (|| {
        // SAFETY: The raw engine handle comes from this module.
        let engine = unsafe { engine_ref(engine) }?;
        let json = serialize_vocabulary(&engine.vocabulary());
        let raw = string_into_raw(json);
        if raw.is_null() {
            return Err("failed to allocate vocabulary string".to_string());
        }
        Ok(raw)
    })();

    match result {
        Ok(value) => value,
        Err(error) => {
            set_last_error(error);
            ptr::null_mut()
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn minisearch_engine_top_terms(
    engine: *const SearchEngine,
    limit: u64,
) -> *mut c_char {
    clear_last_error();

    let result = (|| {
        // SAFETY: The raw engine handle comes from this module.
        let engine = unsafe { engine_ref(engine) }?;
        let stats = engine.top_terms(usize_from_u64(limit, "limit")?);
        let json = serialize_term_stats(&stats);
        let raw = string_into_raw(json);
        if raw.is_null() {
            return Err("failed to allocate term stats string".to_string());
        }
        Ok(raw)
    })();

    match result {
        Ok(value) => value,
        Err(error) => {
            set_last_error(error);
            ptr::null_mut()
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn minisearch_engine_documents(engine: *const SearchEngine) -> *mut c_char {
    clear_last_error();

    let result = (|| {
        // SAFETY: The raw engine handle comes from this module.
        let engine = unsafe { engine_ref(engine) }?;
        let json = serialize_documents(engine.documents());
        let raw = string_into_raw(json);
        if raw.is_null() {
            return Err("failed to allocate documents string".to_string());
        }
        Ok(raw)
    })();

    match result {
        Ok(value) => value,
        Err(error) => {
            set_last_error(error);
            ptr::null_mut()
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn minisearch_engine_document(
    engine: *const SearchEngine,
    doc_id: u64,
) -> *mut c_char {
    clear_last_error();

    let result = (|| {
        // SAFETY: The raw engine handle comes from this module.
        let engine = unsafe { engine_ref(engine) }?;
        let doc_id = usize_from_u64(doc_id, "doc_id")?;
        let json = match engine.document(doc_id) {
            Some(document) => serialize_document(document),
            None => "null".to_string(),
        };
        let raw = string_into_raw(json);
        if raw.is_null() {
            return Err("failed to allocate document string".to_string());
        }
        Ok(raw)
    })();

    match result {
        Ok(value) => value,
        Err(error) => {
            set_last_error(error);
            ptr::null_mut()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_escape_handles_control_characters() {
        let mut output = String::new();
        json_escape("line\tone\nzero\0", &mut output);
        assert_eq!(output, "\"line\\tone\\nzero\\u0000\"");
    }

    #[test]
    fn ffi_search_returns_json_payload() {
        let engine = minisearch_engine_new();
        let path = CString::new("guide.txt").unwrap();
        let content = CString::new("Rust search engine with snippets").unwrap();
        let query = CString::new("rust").unwrap();

        let added =
            unsafe { minisearch_engine_add_document(engine, path.as_ptr(), content.as_ptr()) };
        assert!(added);

        let results =
            unsafe { minisearch_engine_search(engine, query.as_ptr(), 5, ptr::null(), false, 0.0) };
        assert!(!results.is_null());

        let json = unsafe { CStr::from_ptr(results) }
            .to_str()
            .unwrap()
            .to_string();
        assert!(json.contains("\"path\":\"guide.txt\""));
        assert!(json.contains("\"matched_terms\":[\"rust\"]"));

        unsafe {
            minisearch_string_free(results);
            minisearch_engine_free(engine);
        }
    }
}
