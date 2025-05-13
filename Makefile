# Load Ants Makefile

# 二进制名称
BINARY_NAME := loadants

# 基本构建命令
CARGO := cargo

# 获取当前系统信息
UNAME_S := $(shell uname -s)
UNAME_M := $(shell uname -m)
DATE := $(shell date +%Y%m%d%H%M%S)

# 默认目标平台（将会根据实际执行环境被覆盖）
TARGET := x86_64-pc-windows-gnu

# 平台特定配置
ifeq ($(OS),Windows_NT)
  # Windows
  TARGET := x86_64-pc-windows-gnu
  EXT := .exe
else
  UNAME_S := $(shell uname -s)
  ifeq ($(UNAME_S),Linux)
    # Linux
    ifeq ($(UNAME_M),arm64)
      TARGET := aarch64-unknown-linux-gnu
    else
      TARGET := x86_64-unknown-linux-gnu
    endif
    EXT :=
  else ifeq ($(UNAME_S),Darwin)
    # MacOS
    ifeq ($(UNAME_M),arm64)
      TARGET := aarch64-apple-darwin
    else
      TARGET := x86_64-apple-darwin
    endif
    EXT :=
  endif
endif

# 输出目录
OUT_DIR := ./target/$(TARGET)/release

# 默认目标
.PHONY: default
default: build

# 构建调试版本
.PHONY: build
build:
	$(CARGO) build --bin $(BINARY_NAME) --target $(TARGET)

# 构建发布版本
.PHONY: build-release
build-release:
	$(CARGO) build --release --bin $(BINARY_NAME) --target $(TARGET)

# 构建发布版本
.PHONY: build-release
build-release:

# 运行代码检查
.PHONY: check
check:
	$(CARGO) fmt -- --check
	$(CARGO) clippy --target $(TARGET) -- -D warnings

# 运行测试
.PHONY: test
test:
	$(CARGO) test --target $(TARGET)

# 清理构建产物
.PHONY: clean
clean:
	$(CARGO) clean

# 帮助信息
.PHONY: help
help:
	@echo "OxideWebDNS Makefile"
	@echo ""
	@echo "Target Platform: $(TARGET)"
	@echo ""
	@echo "Usage:"
	@echo "  make              - 构建发布版本 (等同于 make build-release)"
	@echo "  make build        - 构建调试版本"
	@echo "  make build-release- 构建带优化的发布版本"
	@echo "  make check        - 运行代码检查 (format, clippy)"
	@echo "  make test         - 运行测试"
	@echo "  make clean        - 清理构建产物"
	@echo "  make help         - 显示帮助信息" 