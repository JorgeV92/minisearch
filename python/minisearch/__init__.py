from __future__ import annotations

import ctypes
import json
import os
import sys
from dataclasses import dataclass
from pathlib import Path

__all__ = [
    "DocumentMeta",
    "IndexOptions",
    "SearchEngine",
    "SearchError",
    "SearchOptions",
    "SearchResult",
    "TermStat",
]


class SearchError(RuntimeError):
    pass


@dataclass
class SearchOptions:
    top_k: int = 10
    path_prefix: str | None = None
    min_score: float | None = None


@dataclass
class IndexOptions:
    extensions: list[str] | None = None
    max_file_size_bytes: int | None = None


@dataclass
class SearchResult:
    doc_id: int
    path: str
    score: float
    matched_terms: list[str]
    snippet: str | None


@dataclass
class TermStat:
    term: str
    document_frequency: int
    total_frequency: int


@dataclass
class DocumentMeta:
    id: int
    path: str
    length: int
    content: str
    extension: str | None
    title: str
    modified_unix_timestamp_secs: int | None


def _library_names() -> list[str]:
    if sys.platform == "darwin":
        return ["libminisearch.dylib"]
    if os.name == "nt":
        return ["minisearch.dll"]
    return ["libminisearch.so"]


def _candidate_library_paths() -> list[Path]:
    names = _library_names()
    candidates: list[Path] = []

    env_path = os.environ.get("MINISEARCH_LIBRARY")
    if env_path:
        candidates.append(Path(env_path).expanduser())

    package_dir = Path(__file__).resolve().parent
    repo_root = package_dir.parent.parent
    search_dirs = [
        package_dir,
        repo_root / "target" / "release",
        repo_root / "target" / "release" / "deps",
        repo_root / "target" / "debug",
        repo_root / "target" / "debug" / "deps",
    ]

    for directory in search_dirs:
        for name in names:
            candidates.append(directory / name)

    return candidates


def _load_library() -> ctypes.CDLL:
    for candidate in _candidate_library_paths():
        if candidate.exists():
            return ctypes.CDLL(str(candidate))

    searched = "\n".join(f"- {path}" for path in _candidate_library_paths())
    raise SearchError(
        "Could not find the minisearch shared library.\n"
        "Build it with `cargo build --release --features python-bindings` and, if needed, set "
        "`MINISEARCH_LIBRARY` to the compiled library path.\n"
        f"Searched:\n{searched}"
    )


_LIB = _load_library()

_LIB.minisearch_engine_new.argtypes = []
_LIB.minisearch_engine_new.restype = ctypes.c_void_p

_LIB.minisearch_engine_free.argtypes = [ctypes.c_void_p]
_LIB.minisearch_engine_free.restype = None

_LIB.minisearch_last_error_message.argtypes = []
_LIB.minisearch_last_error_message.restype = ctypes.c_void_p

_LIB.minisearch_string_free.argtypes = [ctypes.c_void_p]
_LIB.minisearch_string_free.restype = None

_LIB.minisearch_engine_add_document.argtypes = [
    ctypes.c_void_p,
    ctypes.c_char_p,
    ctypes.c_char_p,
]
_LIB.minisearch_engine_add_document.restype = ctypes.c_bool

_LIB.minisearch_engine_build_from_directory.argtypes = [
    ctypes.c_char_p,
    ctypes.c_bool,
    ctypes.POINTER(ctypes.c_char_p),
    ctypes.c_uint64,
    ctypes.c_bool,
    ctypes.c_uint64,
]
_LIB.minisearch_engine_build_from_directory.restype = ctypes.c_void_p

_LIB.minisearch_engine_index_directory.argtypes = [
    ctypes.c_void_p,
    ctypes.c_char_p,
    ctypes.c_bool,
    ctypes.POINTER(ctypes.c_char_p),
    ctypes.c_uint64,
    ctypes.c_bool,
    ctypes.c_uint64,
]
_LIB.minisearch_engine_index_directory.restype = ctypes.c_int64

_LIB.minisearch_engine_search.argtypes = [
    ctypes.c_void_p,
    ctypes.c_char_p,
    ctypes.c_uint64,
    ctypes.c_char_p,
    ctypes.c_bool,
    ctypes.c_double,
]
_LIB.minisearch_engine_search.restype = ctypes.c_void_p

_LIB.minisearch_engine_save_to_path.argtypes = [ctypes.c_void_p, ctypes.c_char_p]
_LIB.minisearch_engine_save_to_path.restype = ctypes.c_bool

_LIB.minisearch_engine_load_from_path.argtypes = [ctypes.c_char_p]
_LIB.minisearch_engine_load_from_path.restype = ctypes.c_void_p

_LIB.minisearch_engine_document_count.argtypes = [ctypes.c_void_p]
_LIB.minisearch_engine_document_count.restype = ctypes.c_uint64

_LIB.minisearch_engine_vocabulary_size.argtypes = [ctypes.c_void_p]
_LIB.minisearch_engine_vocabulary_size.restype = ctypes.c_uint64

_LIB.minisearch_engine_average_document_length.argtypes = [ctypes.c_void_p]
_LIB.minisearch_engine_average_document_length.restype = ctypes.c_double

_LIB.minisearch_engine_vocabulary.argtypes = [ctypes.c_void_p]
_LIB.minisearch_engine_vocabulary.restype = ctypes.c_void_p

_LIB.minisearch_engine_top_terms.argtypes = [ctypes.c_void_p, ctypes.c_uint64]
_LIB.minisearch_engine_top_terms.restype = ctypes.c_void_p

_LIB.minisearch_engine_documents.argtypes = [ctypes.c_void_p]
_LIB.minisearch_engine_documents.restype = ctypes.c_void_p

_LIB.minisearch_engine_document.argtypes = [ctypes.c_void_p, ctypes.c_uint64]
_LIB.minisearch_engine_document.restype = ctypes.c_void_p


def _read_error_message() -> str:
    pointer = _LIB.minisearch_last_error_message()
    if not pointer:
        return "unknown minisearch error"
    try:
        return ctypes.string_at(pointer).decode("utf-8")
    finally:
        _LIB.minisearch_string_free(pointer)


def _raise_last_error(prefix: str) -> None:
    raise SearchError(f"{prefix}: {_read_error_message()}")


def _consume_json_pointer(pointer: int | None, prefix: str):
    if not pointer:
        _raise_last_error(prefix)
    try:
        return json.loads(ctypes.string_at(pointer).decode("utf-8"))
    finally:
        _LIB.minisearch_string_free(pointer)


def _encode(value: str) -> bytes:
    return value.encode("utf-8")


def _prepare_extensions(
    options: IndexOptions | None,
) -> tuple[bool, ctypes.Array[ctypes.c_char_p] | None, int]:
    if options is None or options.extensions is None:
        return False, None, 0

    encoded = [_encode(extension) for extension in options.extensions]
    array = (ctypes.c_char_p * len(encoded))(*encoded)
    return True, array, len(encoded)


def _max_file_size(options: IndexOptions | None) -> tuple[bool, int]:
    if options is None or options.max_file_size_bytes is None:
        return False, 0
    return True, int(options.max_file_size_bytes)


def _search_options(options: SearchOptions | None) -> SearchOptions:
    return options if options is not None else SearchOptions()


def _term_stat_from_payload(payload: dict) -> TermStat:
    return TermStat(
        term=payload["term"],
        document_frequency=payload["document_frequency"],
        total_frequency=payload["total_frequency"],
    )


def _document_from_payload(payload: dict | None) -> DocumentMeta | None:
    if payload is None:
        return None
    return DocumentMeta(
        id=payload["id"],
        path=payload["path"],
        length=payload["length"],
        content=payload["content"],
        extension=payload["extension"],
        title=payload["title"],
        modified_unix_timestamp_secs=payload["modified_unix_timestamp_secs"],
    )


def _search_result_from_payload(payload: dict) -> SearchResult:
    return SearchResult(
        doc_id=payload["doc_id"],
        path=payload["path"],
        score=payload["score"],
        matched_terms=list(payload["matched_terms"]),
        snippet=payload["snippet"],
    )


class SearchEngine:
    def __init__(self, _handle: int | None = None) -> None:
        handle = _handle if _handle is not None else _LIB.minisearch_engine_new()
        if not handle:
            _raise_last_error("failed to create search engine")
        self._handle = handle

    def _require_handle(self) -> int:
        if not self._handle:
            raise SearchError("search engine is closed")
        return self._handle

    @classmethod
    def build_from_directory(
        cls,
        directory: str | os.PathLike[str],
        options: IndexOptions | None = None,
    ) -> "SearchEngine":
        has_extensions, extensions, extensions_len = _prepare_extensions(options)
        has_max_file_size, max_file_size = _max_file_size(options)
        handle = _LIB.minisearch_engine_build_from_directory(
            _encode(os.fspath(directory)),
            has_extensions,
            extensions,
            extensions_len,
            has_max_file_size,
            max_file_size,
        )
        if not handle:
            _raise_last_error("failed to build index from directory")
        return cls(_handle=handle)

    @classmethod
    def load_from_path(cls, path: str | os.PathLike[str]) -> "SearchEngine":
        handle = _LIB.minisearch_engine_load_from_path(_encode(os.fspath(path)))
        if not handle:
            _raise_last_error("failed to load index")
        return cls(_handle=handle)

    def close(self) -> None:
        if getattr(self, "_handle", None):
            _LIB.minisearch_engine_free(self._handle)
            self._handle = None

    def __enter__(self) -> "SearchEngine":
        return self

    def __exit__(self, exc_type, exc, tb) -> None:
        self.close()

    def __del__(self) -> None:
        try:
            self.close()
        except Exception:
            pass

    def add_document(self, path: str, content: str) -> None:
        ok = _LIB.minisearch_engine_add_document(
            self._require_handle(),
            _encode(path),
            _encode(content),
        )
        if not ok:
            _raise_last_error("failed to add document")

    def index_directory(
        self,
        directory: str | os.PathLike[str],
        options: IndexOptions | None = None,
    ) -> int:
        has_extensions, extensions, extensions_len = _prepare_extensions(options)
        has_max_file_size, max_file_size = _max_file_size(options)
        count = _LIB.minisearch_engine_index_directory(
            self._require_handle(),
            _encode(os.fspath(directory)),
            has_extensions,
            extensions,
            extensions_len,
            has_max_file_size,
            max_file_size,
        )
        if count < 0:
            _raise_last_error("failed to index directory")
        return int(count)

    def search(
        self,
        query: str,
        options: SearchOptions | None = None,
    ) -> list[SearchResult]:
        resolved = _search_options(options)
        pointer = _LIB.minisearch_engine_search(
            self._require_handle(),
            _encode(query),
            int(resolved.top_k),
            None if resolved.path_prefix is None else _encode(resolved.path_prefix),
            resolved.min_score is not None,
            0.0 if resolved.min_score is None else float(resolved.min_score),
        )
        payload = _consume_json_pointer(pointer, "failed to search index")
        return [_search_result_from_payload(result) for result in payload]

    def save_to_path(self, path: str | os.PathLike[str]) -> None:
        ok = _LIB.minisearch_engine_save_to_path(
            self._require_handle(),
            _encode(os.fspath(path)),
        )
        if not ok:
            _raise_last_error("failed to save index")

    def document_count(self) -> int:
        return int(_LIB.minisearch_engine_document_count(self._require_handle()))

    def vocabulary_size(self) -> int:
        return int(_LIB.minisearch_engine_vocabulary_size(self._require_handle()))

    def average_document_length(self) -> float:
        return float(_LIB.minisearch_engine_average_document_length(self._require_handle()))

    def vocabulary(self) -> list[str]:
        payload = _consume_json_pointer(
            _LIB.minisearch_engine_vocabulary(self._require_handle()),
            "failed to read vocabulary",
        )
        return list(payload)

    def top_terms(self, limit: int) -> list[TermStat]:
        payload = _consume_json_pointer(
            _LIB.minisearch_engine_top_terms(self._require_handle(), int(limit)),
            "failed to read top terms",
        )
        return [_term_stat_from_payload(item) for item in payload]

    def documents(self) -> list[DocumentMeta]:
        payload = _consume_json_pointer(
            _LIB.minisearch_engine_documents(self._require_handle()),
            "failed to read documents",
        )
        return [_document_from_payload(item) for item in payload]

    def document(self, doc_id: int) -> DocumentMeta | None:
        payload = _consume_json_pointer(
            _LIB.minisearch_engine_document(self._require_handle(), int(doc_id)),
            "failed to read document",
        )
        return _document_from_payload(payload)
