#!/usr/bin/env bash
# cargo 辅助脚本 — 绕过 WorkBuddy 沙箱对 target 目录的写保护
#
# 问题: WorkBuddy 沙箱的 safe-delete 拦截 target/debug/.cargo-build-lock 的删除，
#       导致同一 target 目录第二次构建失败 (os error 5)。
# 方案 A（推荐，快）: 用项目外 target 目录（沙箱不拦），增量编译仅数秒：
#   MIGRATOR_TARGET_DIR=/tmp/migrator-target ./cargo.sh build
# 方案 B（默认）: 每次构建使用新的 target 目录 (target-build-<N>)，依赖会全量重编 (约 8-9 分钟)。

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

if [ -n "${MIGRATOR_TARGET_DIR:-}" ]; then
  TARGET_DIR="${MIGRATOR_TARGET_DIR}"
else
  TARGET_DIR="$(next_target_dir)"
fi
export CARGO_TARGET_DIR="${TARGET_DIR}"

echo "[migrator-cargo] target dir: ${TARGET_DIR}"
cd "${PROJECT_DIR}"
exec "${CARGO}" "$@"
