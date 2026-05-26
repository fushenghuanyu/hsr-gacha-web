# hsr-gacha-web

《崩坏：星穹铁道》Web 版抽卡分析工具。前端负责展示与本地缓存，Rust 后端负责拉取官方跃迁记录、历史卡池 UP 判定与用户数据持久化。

## 环境依赖

| 依赖 | 用途 | 建议版本 |
|------|------|----------|
| [Node.js](https://nodejs.org/) | 前端依赖安装与 Vite 构建 | 18.x 或 20.x LTS |
| npm | 随 Node.js 安装 | 9+ |
| [Rust 工具链](https://www.rust-lang.org/tools/install) | 编译后端 API 与 GUI 启动器 | stable，edition 2021 |

- HTTP 客户端使用 `rustls`，无需单独安装 OpenSSL。
- GUI 启动器基于 OpenGL；Linux 若缺少驱动，可改用命令行二进制 `hsr-gacha-api`。
- 可选：在项目根维护 `VERSION` 文件注入版本号；缺失时后端回退为 `Cargo.toml` 包版本。

## 目录结构

```
hsr-gacha-web/
├── frontend/       # React + Vite 前端（开发端口 5173）
├── backend-rust/   # Rust Axum API 与 GUI 启动器（默认端口 8000）
├── scripts/        # 构建脚本（release.mjs）
├── resources/      # 卡池 JSON、名称字典、角色/光锥图标
├── dist/           # 前端构建产物（npm run build 生成，不提交 git）
└── out/            # 一键构建分发目录（不提交 git）
```

## 功能说明

支持三种数据来源：

1. **手动链接**：粘贴游戏内「跃迁记录」网页链接（含 `authkey`），由后端在线拉取。
2. **自动拉取**：后端读取本机 `Player.log` 与 webCaches 缓存自动提取链接（需本机已运行过游戏）。
3. **UIGF 导入**：前端导入 UIGF 格式 JSON，在浏览器本地解析分析。

历史卡池 UP 数据位于 `resources/gacha/`（`character.json`、`weapon.json`），请直接维护这两个文件。

## 一键构建（推荐）

在项目根目录执行，自动编译前端与启动器、组装分发目录并打 zip：

```bash
node scripts/release.mjs
```

常用选项：

```bash
node scripts/release.mjs --npm-install   # 强制 npm install
node scripts/release.mjs --no-build      # 不重新编译，仅复制/打包现有产物
node scripts/release.mjs --skip-zip      # 只输出 out/，不打 zip
```

### 构建产物布局

```
out/
├── hsr-gacha-launcher.exe       # 启动器（Windows；Linux/macOS 无 .exe 后缀）
├── dist/                        # 前端静态页面
├── resources/                   # 卡池、字典、图标
├── VERSION                      # 版本号
├── userData/                    # 运行时生成，重新构建时保留
└── zip/
    └── hsr-gacha-distribution-v{版本}.zip
```

### 构建后如何运行

| 项目 | 位置 |
|------|------|
| **启动器 exe** | `out/hsr-gacha-launcher.exe`（与 `dist/`、`resources/` 同级） |
| **userData** | `out/userData/`（首次保存分析数据后自动创建；再次构建不会删除） |
| **运行日志** | `out/hsr-gacha-launcher.log`（与 exe 同目录，GUI 启动器写入） |

双击 exe 或在 `out/` 目录运行启动器，点击「自动打开浏览器」。后端挂载 `out/dist/` 提供完整页面，浏览器访问 http://127.0.0.1:8000/ 。

## 手动构建

```bash
cd frontend
npm install
npm run build

cd ../backend-rust
cargo build --release
```

编译产物位于 `backend-rust/target/release/`：

- `hsr-gacha-api` — 纯命令行 HTTP 服务（`cargo run --release` 默认）
- `hsr-gacha-launcher` — GUI 启动器

## 运行方式（开发 / 源码目录）

### 命令行仅后端

先完成前端构建（项目根 `dist/` 存在），再启动 API。浏览器访问 http://127.0.0.1:8000/ 。

```bash
cd backend-rust
cargo run --release
```

### GUI 启动器

先完成前端构建（项目根 `dist/` 存在）。浏览器访问 http://127.0.0.1:8000/ 。

```bash
cd backend-rust
cargo run --release --bin hsr-gacha-launcher
```

源码目录下运行时，`userData/` 位于项目根；日志写入 `backend-rust/target/release/hsr-gacha-launcher.log`（若以 release 二进制运行启动器）。

### 前后端分离开发

**终端 A — 后端**

```bash
cd backend-rust
cargo run --release --bin hsr-gacha-api
```

**终端 B — 前端**

```bash
cd frontend
npm install
npm run dev
```

浏览器访问 http://127.0.0.1:5173/ 。页面由 Vite 提供，API 请求后端 8000 端口；此模式下无需构建 `dist/` 。

## 常见问题

- **端口占用**：本项目固定使用 `8000`，被占用时需先关闭占用进程。
- **页面空白（仅后端）**：确认已执行 `npm run build`，且存在 `dist/index.html`（或 `out/dist/index.html`）。
- **自动拉取失败**：确认游戏在本机运行过，且 `Player.log` / 缓存路径可读。
