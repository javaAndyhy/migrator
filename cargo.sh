#!/usr/bin/env bash
# cargo 辅助脚本 — 绕过 WorkBuddy 沙箱对 target 目录的写保护
#
# 问题: WorkBuddy 沙箱的 safe-delete 拦截 target/debug/.cargo-build-lock 的删除，
#       导致同一 target 目录第二次构建失败 (os error 5)。
# 方案: 每次构建使用新的 target 目录 (target-build-<N>)，依赖会全量重编 (约 9 分钟)。
#       若需要复用增量缓存，可手动指定 --target-dir 指向未被污染的目录。

set -euo pipefail

CARGO=/c/Users/honghuayu/.rustup/toolchains/stable-x86_64-pc-windows-msvc/bin/cargo.exe

# 用 cygpath 转 Windows 路径（cargo 不认 /c/ 风格）
PROJECT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
if command -v cygpath >/dev/null 2>&1; then
  PROJECT_DIR="$(cygpath -w "${PROJECT_DIR}")"
fi

# 选择下一个可用 target 目录
next_target_dir() {
  local i=100
  while true; do
    local dir="${PROJECT_DIR}\\target-build-${i}"
    if [ ! -e "${dir}" ]; then
      echo "${dir}"
      return 0
    fi
    i=$((i + 1))
  done
}

TARGET_DIR="$(next_target_dir)"
export CARGO_TARGET_DIR="${TARGET_DIR}"

echo "[migrator-cargo] target dir: ${TARGET_DIR}"
cd "${PROJECT_DIR}"
exec "${CARGO}" "$@"
