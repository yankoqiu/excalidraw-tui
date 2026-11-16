# excalidraw-tui (`exv`)

[English](README.md) | [简体中文](README.zh-CN.md)

在终端里实时预览 Excalidraw，不落任何磁盘临时文件。把 `exv` 指向一个
`.excalidraw` 文件，将终端窗格放在 AI 编程助手旁边，就能在智能体写入文件时
同步看到图表更新。平移、缩放、检查图表，全程不离开终端。

```text
demo.excalidraw | 120% | 10 elements | kitty | Ready | hjkl pan  +/- zoom  0 reset  q quit
```

## 为什么选择 exv

- **绝对零磁盘临时文件。** SVG 通过管道从 JavaScript 渲染器传到 Rust；像素直接
  送往终端。`exv` 不会写入 `.svg`、`.png` 或任何缓存文件。
- **常驻渲染器。** 单个 Node/Bun 进程只加载一次 Excalidraw 渲染器，之后通过
  按行分隔的 JSON-RPC 转换文档，因此刷新很快。
- **兼容原子保存。** 编辑器和智能体通常先写临时文件再重命名覆盖。`exv` 监听
  父目录，做 300 毫秒防抖并校验 JSON；写入不完整时保留上一帧正常画面。
- **中文/CJK 友好。** 渲染器会重写字体栈，光栅化器使用系统字体回退，中文标签
  不会变成方框乱码。
- **不残留子进程。** 无论正常退出、panic 展开，还是关闭终端窗格触发 `SIGHUP`，
  Worker 都会被强制终止并回收。
- **兼容多种终端。** 支持 Kitty 图形、Sixel 或 Unicode 半块，自动探测，也可手动
  指定。

## 环境要求

- Node.js >= 22.22.2（推荐 Node 24 LTS 或 26）
- 支持 Kitty 图形、Sixel，或任意真彩色终端（半块降级）

发布到 npm 的包已包含预编译的 `exv` 二进制和打包后的渲染器，因此用户不需要
Rust 工具链、C 编译器，也不需要安装渲染器依赖。

| 终端 | 常用路径 |
| --- | --- |
| Kitty、Ghostty、WezTerm | Kitty 图形协议 |
| iTerm2、foot、mlterm、WezTerm | Sixel |
| Zed / Herdr 窗格 | 宿主终端协议透传（若支持） |
| 其他终端 | Unicode 半块降级 |

自动探测不准确时，使用 `--protocol` 指定协议。

## 安装

无需全局安装，直接运行：

```sh
npx --yes excalidraw-tui@latest diagram.excalidraw
```

或者全局安装 `exv` 命令：

```sh
npm install -g excalidraw-tui
exv diagram.excalidraw
```

npm 包通过 optional dependencies 为当前平台选择预编译二进制：

| 操作系统 | 架构 | 二进制说明 |
| --- | --- | --- |
| Linux | x64、arm64 | 使用 musl 静态链接 |
| macOS | Intel x64、Apple Silicon arm64 | 原生 |
| Windows | x64 | 原生 |

请保留 optional dependencies。使用 `--omit=optional` 会跳过平台二进制，
`exv` 会提示如何重新安装。

更新：

```sh
npx --yes excalidraw-tui@latest diagram.excalidraw  # 每次获取最新版本
npm update -g excalidraw-tui                        # 更新全局安装
```

如果没有适用于你的系统或架构的预编译二进制，可以从源码构建，或用 `EXV_BIN`
指向自定义的 `exv` 二进制。

### 从源码安装

这种方式面向开发者和暂不支持的平台，需要 Rust 1.88+ 和 Node.js >= 22.22.2。

```sh
git clone https://github.com/iam-yanko/excalidraw-tui.git
cd excalidraw-tui
./install.sh
```

`install.sh` 会执行 `cargo build --release --locked`，把二进制与渲染器安装到
`${PREFIX:-$HOME/.local}/share/excalidraw-tui/`，并把 `exv` 软链接到
`${BIN_DIR:-$PREFIX/bin}`。如果工作区已有 `renderer/node_modules`，会直接复制；
否则使用 Bun 或 npm 安装生产依赖。

可选环境变量：

```sh
PREFIX=/usr/local ./install.sh
BIN_DIR="$HOME/bin" ./install.sh
EXV_FORCE_DEPS=1 ./install.sh   # 重新安装渲染器依赖
```

手动构建：

```sh
npm ci --prefix renderer
cargo build --release --locked
./target/release/exv examples/demo.excalidraw
```

Windows PowerShell：

```powershell
npm ci --prefix renderer
cargo build --release --locked
.\target\release\exv.exe examples\demo.excalidraw
```

二进制会在自身旁边、`../share/excalidraw-tui/` 或本仓库的 `renderer/` 中查找
渲染器。如果目录结构不同，请用 `EXV_RENDERER` 指向具体的 `worker.mjs`；用
`EXV_JS_RUNTIME` 可强制指定 Node 或 Bun 可执行文件。npm 启动器会自动设置这两个
变量。

## 使用方式

```text
exv [OPTIONS] <FILE>
```

```sh
exv diagram.excalidraw
exv --theme dark --protocol kitty architecture.excalidraw
exv --no-watch diagram.excalidraw
npx --yes excalidraw-tui@latest --theme dark diagram.excalidraw
```

| 参数 | 说明 |
| --- | --- |
| `--theme <light\|dark>` | 浅色或深色画布（默认 `light`） |
| `--no-watch` | 只加载一次；要求文件已存在且 JSON 有效 |
| `--protocol <auto\|kitty\|sixel\|halfblocks>` | 强制指定图形协议 |

监听模式下文件可以尚不存在：父目录必须存在，看到第一次有效保存前状态栏显示
`waiting for <name>`。`exv` 必须在交互式终端中启动（stdin 和 stdout 都必须是
TTY）。

### 按键

| 按键 | 操作 |
| --- | --- |
| `h` `j` `k` `l` / 方向键 | 向左/下/上/右平移 |
| `+` 或 `=` | 放大 20%（重新光栅化 SVG） |
| `-` 或 `_` | 缩小 20% |
| `0` 或 `r` | 重置缩放与平移 |
| `q`、`Esc`、`Ctrl+C` | 退出 |

状态栏显示文件名、缩放百分比、元素数量、当前协议、加载/同步状态和按键提示。

## 实时预览工作流

1. 在 AI 编程助手旁边的终端窗格中执行 `exv diagram.excalidraw`。
2. 让智能体创建或更新该文件。
3. 监听器发现原子保存、校验 JSON，然后替换画面。写入不完整时显示
   `Syncing...`，并保留上一张正常图像。
4. 如需修正，直接改文件；下一次有效保存会自动显示。

`exv` 对图表文件只读，绝不会改写你的 `.excalidraw`。

## AI 智能体集成

可直接复制到项目中的规则位于 `integrations/`：

- `integrations/CLAUDE.md` —— Claude Code
- `integrations/AGENTS.md` —— 通用智能体格式
- `integrations/qoder-rules.md` —— Qoder 项目规则

请将这些内容合并进项目说明，而不是粗暴替换现有规则。规则涵盖 JSON 骨架、
元素与绑定字段、可读性布局限制、校验方式以及如何用 `exv` 预览。

示例提示词：

> 创建 `architecture.excalidraw`，展示从 CLI 到数据库的请求路径。节点不超过
> 10 个，并校验 JSON。我会用 exv 预览。

## Herdr 集成

`herdr-plugin.toml` 提供一个弹出式预览窗格和 `preview` 动作。请先全局安装
`exv`，让窗格脚本能从 `PATH` 找到它：

```sh
npm install -g excalidraw-tui
herdr plugin link "$PWD"     # 将当前仓库链接为本地插件
herdr plugin action invoke excalidraw-tui.preview
```

窗格脚本 `scripts/herdr-preview.sh` 优先使用 `EXV_FILE`；未设置时会在
`HERDR_PLUGIN_CONTEXT_JSON` 描述的工作区中查找最新的 `.excalidraw` 文件，最后
回退到插件目录下的 `diagram.excalidraw`。`EXV_BIN` 可指定不在 PATH 中的二进制。
如果经常使用，可以在 Herdr 配置中为该动作绑定快捷键。

## 实现原理

```text
 .excalidraw 文件
      │  监听父目录、300 ms 防抖、JSON 有效性检查
      ▼
 Rust TUI（ratatui + crossterm）
      │  通过 stdin 发送 {"id":N,"json":"..."}
      ▼
 常驻 Node/Bun Worker（打包后的 @excalidraw/utils + jsdom 兼容层）
      │  通过 stdout 返回 {"id":N,"svg":"...","elements":N}
      ▼
 resvg/usvg 矢量树 + 系统 CJK 字体回退
      │  仅视口大小的 RGBA 位图（缩放/平移在此应用）
      ▼
 ratatui-image → Kitty / Sixel / 半块 → 终端
```

| 路径 | 职责 |
| --- | --- |
| `src/main.rs` | CLI、终端会话 RAII、协议选择、信号处理 |
| `src/converter.rs` | Worker 启动/握手/超时；`Drop` 中 kill 并 wait |
| `src/rasterizer.rs` | SVG 解析一次、视口渲染、明暗主题 |
| `src/ui.rs` | 应用状态、按键、状态栏、图像同步 |
| `src/watcher.rs` | 带防抖的父目录监听与文件名过滤 |
| `renderer/worker.mjs` | jsdom 环境、元素归一化、`exportScene` |
| `npm/cli.js` | 选择平台二进制，并以继承的标准流启动 |
| `scripts/prepare-npm.mjs` | 组装并校验 npm tarball |
| `tests/` | 中文全链路、Worker 错误恢复、原子重命名 |

发布前会用 esbuild 把渲染器打包成单个文件，用户无需下载 `@excalidraw/utils`、
jsdom 及其依赖树。只光栅化当前可见视口，且每次文件变化只解析一次 SVG，因此
缩放和平移不会再次调用 JavaScript Worker。

### 零磁盘与进程生命周期保证

- 不写任何临时 SVG/PNG/缓存文件：SVG 文本和像素都留在内存中。
- 用世代计数器丢弃快速连续保存产生的过期转换结果。
- `Converter::drop` 会关闭 stdin、终止子进程并等待回收。Unix 构建还会把
  `SIGHUP`/`SIGINT`/`SIGQUIT`/`SIGTERM` 转换为有序退出。
- 终端原始模式和备用屏幕通过 RAII 恢复，错误路径也不例外。

## 开发

```sh
npm ci --prefix renderer
npm run bundle --prefix renderer
npm run smoke --prefix renderer
node --test tests/npm-cli.test.mjs
cargo test --locked                  # 单元测试 + watcher + 全链路测试
cargo clippy --locked --all-targets -- -D warnings
cargo fmt --check
```

全链路测试需要 Node 或 Bun，以及 `renderer/node_modules`；否则会打印原因并
跳过。测试会转换 `tests/data/sample.excalidraw` 中的中文样例，检查单次请求错误
路径，验证 Worker 在收到畸形文档后仍能继续工作，并在内存中完成 SVG 光栅化。

`Cargo.lock` 有意将 `notify-types` 固定在 `2.2.0`：`2.3.x` 要求 Rust 1.91。
请使用 `--locked` 构建；如需升级，请换用更新的工具链。

本地预览 npm 产物：

```sh
cargo build --release --locked
node scripts/prepare-npm.mjs --local --pack
```

tarball 位于 `dist/npm/tarballs/`；脚本会用 `npm pack --dry-run` 校验内容。
正式发布流程见 [RELEASING.md](RELEASING.md)。

## 故障排查

| 现象 | 处理方式 |
| --- | --- |
| 中文显示为方框 | 安装 CJK 系统字体，例如 Noto Sans CJK SC 或思源黑体；光栅化器会在已安装字体间回退 |
| 提示 `Node.js >=22.22.2 is required` | 升级 Node；渲染器依赖较新的 jsdom |
| 平台包缺失 | 不要使用 `--omit=optional` 重新安装；检查 `npm config get omit`，然后运行 `npm install -g excalidraw-tui@latest` |
| `unsupported platform ...` | 使用受支持的平台、从源码构建，或用 `EXV_BIN` 指向自定义二进制 |
| 提示 `must run in an interactive terminal` | 直接在终端中运行 `exv`；stdin 和 stdout 不能重定向到管道 |
| 一直显示 `waiting for ...` | 父目录必须存在；并把有效 JSON 保存到确切路径 |
| 一直停在 `Syncing...` | 当前文件 JSON 无效或被截断；运行 `jq . file.excalidraw` 检查 |
| 渲染器无法启动 | 检查 `node --version`；源码构建请运行 `npm ci --prefix renderer`；或设置 `EXV_RENDERER` |
| Herdr 中找不到 `exv` | 使用全局 npm 安装，让窗格脚本能从 `PATH` 找到 `exv` |
| 图像错位或乱码 | 先试 `--protocol halfblocks`，再试 `sixel` / `kitty`；部分终端会错误报告字符单元格尺寸 |
| 图像不刷新 | 确认编辑的是同一文件名；监听器按文件名过滤并监听目录 |
| 构建因 `notify-types` 的 MSRV 失败 | 使用仓库中的 `Cargo.lock` 配合 Rust 1.88+，或升级 Rust |

## 限制

- 不内嵌 Excalidraw 自定义字体，改用系统字体并带 CJK 回退。
- 图片元素只有在 `files` 中包含 base64 数据时才能渲染。
- 超大型文档的光栅像素上限为 4000 万，以保护内存。
- 预编译 npm 二进制覆盖 Linux x64/arm64、macOS x64/arm64 和 Windows x64；
  Windows arm64 需要从源码构建或使用 `EXV_BIN`。
- Herdr 插件的 shell 入口面向 Linux 和 macOS。

## 许可证

MIT
