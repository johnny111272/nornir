//! File classification by content and path.
//!
//! Every file is classified exactly once before any checks run.
//! The classification determines which checks apply (via the matrix).

use crate::structures::FileKind;

/// Classify a Python source file into one of 8 kinds.
///
/// Decision tree (order matters):
/// 1. PEP 723 shebang → Script
/// 2. "test" in filename → Test
/// 3. Zone path → DataStructure / UnsafeImpure / UnsafePure / ImpureFunction / PureFunction
/// 4. Fallback → Outside
pub fn classify_file(file_path: &str, first_line: &str) -> FileKind {
    if first_line.starts_with("#!/usr/bin/env -S uv run") {
        return FileKind::Script;
    }

    let filename = file_path.rsplit('/').next().unwrap_or(file_path);
    if filename.contains("test") {
        return FileKind::Test;
    }

    if file_path.contains("/structures/") {
        return FileKind::DataStructure;
    }
    if file_path.contains("/unsafe/impure/") {
        return FileKind::UnsafeImpure;
    }
    if file_path.contains("/unsafe/pure/") {
        return FileKind::UnsafePure;
    }
    if file_path.contains("/functions/impure/") {
        return FileKind::ImpureFunction;
    }
    if file_path.contains("/functions/pure/") {
        return FileKind::PureFunction;
    }

    FileKind::Outside
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_detected_by_shebang() {
        assert_eq!(
            classify_file("/project/tools/run.py", "#!/usr/bin/env -S uv run"),
            FileKind::Script,
        );
    }

    #[test]
    fn shebang_takes_priority_over_path() {
        assert_eq!(
            classify_file(
                "/project/src/pkg/functions/pure/tool.py",
                "#!/usr/bin/env -S uv run",
            ),
            FileKind::Script,
        );
    }

    #[test]
    fn test_file_detected_by_name() {
        assert_eq!(
            classify_file("/project/tests/test_foo.py", "import pytest"),
            FileKind::Test,
        );
    }

    #[test]
    fn test_in_name_takes_priority_over_path() {
        assert_eq!(
            classify_file(
                "/project/src/pkg/structures/test_models.py",
                "from pydantic import BaseModel",
            ),
            FileKind::Test,
        );
    }

    #[test]
    fn structures_path() {
        assert_eq!(
            classify_file("/project/src/pkg/structures/models.py", "from pydantic import BaseModel"),
            FileKind::DataStructure,
        );
    }

    #[test]
    fn unsafe_impure_path() {
        assert_eq!(
            classify_file("/project/src/pkg/unsafe/impure/io.py", "import os"),
            FileKind::UnsafeImpure,
        );
    }

    #[test]
    fn unsafe_pure_path() {
        assert_eq!(
            classify_file("/project/src/pkg/unsafe/pure/cast.py", "from typing import cast"),
            FileKind::UnsafePure,
        );
    }

    #[test]
    fn functions_impure_path() {
        assert_eq!(
            classify_file("/project/src/pkg/functions/impure/loader.py", "import json"),
            FileKind::ImpureFunction,
        );
    }

    #[test]
    fn functions_pure_path() {
        assert_eq!(
            classify_file("/project/src/pkg/functions/pure/compute.py", "def add(a, b):"),
            FileKind::PureFunction,
        );
    }

    #[test]
    fn outside_file() {
        assert_eq!(
            classify_file("/project/src/app.py", "import sys"),
            FileKind::Outside,
        );
    }
}
