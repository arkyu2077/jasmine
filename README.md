<p align="center">
  <img src="src-tauri/icons/128x128@2x.png" width="88" alt="Jasmine" />
</p>

<h1 align="center">Jasmine</h1>

<p align="center">
  <b>Codex + an infinite canvas. Unleash your creativity.</b><br/>
  Edit video, edit images, and more — your local Codex, set loose on a canvas without limits.
</p>

<p align="center">
  <a href="https://github.com/arkyu2077/jasmine/releases/latest"><b>⬇ Download</b></a> &nbsp;·&nbsp;
  <a href="#using-jasmine">How to use</a> &nbsp;·&nbsp;
  <a href="#中文">中文</a> &nbsp;·&nbsp;
  <a href="./LICENSE">License (AGPL-3.0)</a>
</p>

---

<a id="english"></a>

## What is Jasmine?

**Codex, on an infinite canvas — your creativity, unleashed.**

The local Codex you already pay for can edit images, edit video, and render motion graphics — but it's stuck in a chat box, and a scrolling column of text is a terrible place to do visual work. You can't point at the part you mean, see three takes side by side, or keep the original next to the edit.

Jasmine sets it loose on an **infinite canvas**. Open a folder and its images and clips spread across the canvas. Point at exactly what you mean, mark it up, say what you want — Codex generates, edits, and animates, and every result lands right beside its source. **You direct; Codex creates.**

No new model, no API key, no per-image fee — Jasmine drives the Codex you already run (your ChatGPT subscription). It owns only what a chat window can't: a spatial canvas, the files that *are* your work, originals that never get overwritten, and a way to point at exactly what you mean.

## Why Jasmine

- **One canvas for stills *and* motion** — generate and edit images, then turn them into clips and motion graphics, all on the same canvas.
- **Point, don't describe** — circle the pixels (or scrub to the moment) you mean; Codex sees exactly what you're pointing at.
- **Non-destructive by default** — originals never change; every version is a new file beside the last, so the layout *is* the history.
- **The Codex you already have** — your ChatGPT subscription. No API key, no extra bill.
- **Local and yours** — your workspace is just a folder of files any tool can open.

---

## Download & install

> **Just want to try it?** Grab the latest build — no source checkout, no toolchain.
>
> ### **→ [Download the latest release](https://github.com/arkyu2077/jasmine/releases/latest)**

### Before you start (required)

Jasmine drives **your own Codex** — it does not bundle or replace it. You need the **Codex CLI installed and signed in once**:

```bash
codex login        # uses your ChatGPT subscription — no API key
codex --version    # confirm it's on your PATH
```

If `codex` isn't installed yet, see the [Codex CLI docs](https://developers.openai.com/codex/cli). Without a signed-in Codex, Jasmine opens but has no agent to talk to.

### macOS

1. On the [releases page](https://github.com/arkyu2077/jasmine/releases/latest), download the `.dmg` for your chip:
   - **Apple Silicon** (M-series): `Jasmine_<version>_aarch64.dmg`
   - **Intel**: `Jasmine_<version>_x64.dmg`
2. Open the `.dmg` and drag **Jasmine** into **Applications**.
3. **First launch** — the build is not yet Apple-notarized, so macOS Gatekeeper will warn "Jasmine can't be opened because Apple cannot check it for malicious software." This is expected. To open it:
   - **Right-click** (or Control-click) **Jasmine.app → Open → Open**, *or*
   - **System Settings → Privacy & Security → "Open Anyway"** after the first blocked attempt.
   - If it still won't open, clear the quarantine flag in Terminal:
     ```bash
     xattr -dr com.apple.quarantine /Applications/Jasmine.app
     ```

   You only have to do this once.

### Windows

> A Windows installer isn't part of **v0.0.1** yet — it's coming in a follow-up release. For now, Windows users can [build from source](#build-from-source).

1. (When available) download the `…_x64-setup.exe` installer from the [releases page](https://github.com/arkyu2077/jasmine/releases/latest).
2. Run it. Because the build isn't code-signed, **SmartScreen** may warn — click **More info → Run anyway**.
3. WebView2 is required (preinstalled on Windows 11; the installer fetches it on Windows 10 if missing).

> **Why the security warnings?** Jasmine is an independent open-source app and isn't yet paid-signed/notarized by Apple or Microsoft. The steps above are the standard way to run an unsigned app you trust. You can always [build it yourself from source](#build-from-source) instead.

---

<a id="using-jasmine"></a>

## Using Jasmine

1. **Open a folder.** Its images and videos appear on the canvas. Drag-and-drop, paste, or "Add images…" to bring more in. The folder *is* where Codex reads and writes — your files stay on disk, readable by any other tool.
2. **Pan / zoom.** Two-finger scroll pans, ⌘/Ctrl + scroll zooms. Click to select, Shift-drag to box-select several at once, drag to move; corner handles resize, the top handle rotates.
3. **Mark a region** (optional). Press `R` (or pick the ▭ tool), drag a box on an image, and add a note. `V` returns to select. Your mark is drawn onto the image and sent to Codex — so you point instead of describe.
4. **Ask Codex.** With image(s) selected, type an instruction (or tap a preset like *Remove background*) and Send. Use the **"+"** menu to attach uploads or pick an enabled Codex plugin for the turn.
5. **Results land to the right of the source.** Keep going — "warmer," "now change the background," "make it a 10-second clip." The session keeps going, your originals are never changed, and every result is a new file sitting next to the one it came from.

## Features

- An infinite **canvas** — pan, zoom, box-select, drag, resize, rotate, minimap, fit-all / zoom-to-selection.
- **Region marks** (rectangle / ellipse / arrow / brush / point) with a note on each, sent to Codex as a marked-up image.
- **Presets** for one-tap operations (remove background, upscale, …) plus free-form instructions.
- **Video & motion** — bundled `ffmpeg` to trim, join, and convert clips, plus an HTML→video renderer; generated clips appear and play **live on the canvas**.
- **Compare** results with a before/after slider or side-by-side view; **crop**, copy, reveal in file manager, export.
- **Undo / redo**, **multiple chats** per folder (each saved with its full history), and a **workspaces** sidebar.
- **One continuous Codex chat** — replies stream in, you can watch its steps, and it can ask clarifying questions.
- **Settings + network proxy**, unified logging, **English / 中文 interface**, and a system tray.
- Platforms: **macOS** — native Apple Silicon and Intel. **Windows is coming next.**

## How it works

Jasmine is **Tauri 2 (Rust) + React + PixiJS v8**. The web layer owns chrome; a GPU compositor owns the canvas. Images are fed to the canvas through a custom Jasmine image protocol with path-normalization and traversal guards; Rust handles decode / downscale / mipmaps off the main thread.

Codex runs as a long-lived **`codex app-server`** sidecar (JSON-RPC 2.0 over stdio) — one process per board, so the session is genuinely stateful. Marked regions are flattened to an overlay image and sent alongside the clean image as file paths the agent reads itself. Image and video outputs are detected, saved as new files, and placed to the right of the source they were made from.

---

<a id="build-from-source"></a>

## Build from source

For developers, or if you'd rather not run an unsigned binary.

**Prerequisites:** the [Codex CLI](https://developers.openai.com/codex/cli) (signed in), Node 20+ with **pnpm**, the **Rust** toolchain (rustup). macOS also needs Xcode Command Line Tools (`xcode-select --install`); Windows needs the *Desktop development with C++* Build Tools + WebView2.

```bash
./setup.sh            # macOS — checks toolchain, adds Rust targets, installs deps
.\setup.ps1           # Windows (PowerShell)

pnpm install
pnpm tauri dev        # run the app live with hot reload
```

### Packaging

Build scripts live at the repo root (`.sh` = macOS, `.ps1` = Windows). Tauri can't cross-compile between macOS and Windows, so each release build runs on its own OS.

| | macOS | Windows |
|---|---|---|
| First-time setup | `./setup.sh` | `.\setup.ps1` |
| Dev build (unsigned, fast) | `./build_dev.sh` → `Jasmine.app` | `.\build_dev.ps1` → `jasmine.exe` |
| Release build | `./build_release.sh` → per-arch `.dmg` | `.\build_release.ps1` → NSIS installer |
| Publish to GitHub Releases | `./publish_release.sh` | `.\publish_release.ps1` |

Release signing/notarization is optional and read from `.env` (macOS) — see [`.env.example`](./.env.example). Without it, builds are unsigned (fine locally; users see the Gatekeeper/SmartScreen prompts documented above).

## Status

**v0.0.1 — the first public release.** The full loop works end-to-end: open a folder → spread it on the canvas → point and mark → ask Codex → the result lands next to the image it came from → keep going. Published to [GitHub Releases](https://github.com/arkyu2077/jasmine/releases). This first build is **macOS** — native Apple Silicon and Intel. **A Windows installer is coming next.**

## Acknowledgments

Jasmine stands on the shoulders of **[Cameo](https://github.com/hAcKlyc/cameo)** — the project that inspired this one. Heartfelt thanks to its authors for the original vision and the groundwork Jasmine builds directly upon. In that spirit, Jasmine keeps the same **AGPL-3.0-or-later** license.

## License & disclaimer

Licensed under **[AGPL-3.0-or-later](./LICENSE)**. You may use, modify, and redistribute Jasmine under its terms; if you run a modified version as a network service, the AGPL requires you to offer your source to its users.

Jasmine is an independent, unofficial tool. It drives the Codex CLI but is **not** affiliated with, endorsed by, or sponsored by OpenAI. "Codex" and related names belong to their respective owners.

---

<a id="中文"></a>

## Jasmine 是什么？

**Codex,搬上无限画布 —— 让你的创意无限释放。**

你已经在付费的本地 Codex，本来就能修图、剪视频、做动效——但它困在聊天框里，而一条不断下滚的文字流，是做视觉最糟糕的地方:你没法指着「就这块」，没法把三个方案并排着看，也没法让原图和改后图挨在一起。

Jasmine 把它放上一块**无限画布**。打开一个文件夹，里面的图和片段在画布上铺开;你指着要改的地方、画个框、说一句 —— Codex 来生成、修改、做动效，每个结果都落在源图旁边。**你来指挥，Codex 来创作。**

不换模型、不要 API key、不按张收费 —— Jasmine 驱动的是你已经在跑的 Codex（你的 ChatGPT 订阅）。它只负责聊天框给不了的:空间画布、**就是你作品本身**的文件、原图永不被改写、改了哪都看得到，以及「指着说」。

## 为什么用 Jasmine

- **图与视频，同一块画布** —— 先生成、修图，再把它们做成片段和动效，全在同一块画布上。
- **指，而不是描述** —— 圈出你要的像素（或拖到某一帧），Codex 看得到你到底指着什么。
- **原图永不被改** —— 每个版本都是上一版旁边的新文件;从哪改来的、改了几版，看位置就知道。
- **用你已有的 Codex** —— 你的 ChatGPT 订阅，无需 API key，不额外花钱。
- **本地、属于你** —— 你的工作区就是一个文件夹，任何工具都能打开。

---

## 下载与安装

> **只想试试？** 直接下编译好的安装包——不用拉源码、不用配工具链。
>
> ### **→ [下载最新版本](https://github.com/arkyu2077/jasmine/releases/latest)**

### 开始前（必需）

Jasmine 驱动的是**你自己的 Codex**——它不打包、也不替代 Codex。你需要先把 **Codex CLI 装好并登录一次**：

```bash
codex login        # 用你的 ChatGPT 订阅，无需 API key
codex --version    # 确认它在 PATH 上
```

若还没装 `codex`，见 [Codex CLI 文档](https://developers.openai.com/codex/cli)。没有已登录的 Codex，Jasmine 能打开但没有 agent 可对话。

### macOS

1. 在[发布页](https://github.com/arkyu2077/jasmine/releases/latest)下载对应芯片的 `.dmg`：
   - **Apple Silicon**（M 系列）：`Jasmine_<版本>_aarch64.dmg`
   - **Intel**：`Jasmine_<版本>_x64.dmg`
2. 打开 `.dmg`，把 **Jasmine** 拖进 **Applications**。
3. **首次打开**——目前还没做 Apple 公证，macOS Gatekeeper 会提示「无法打开，因为 Apple 无法检查其是否包含恶意软件」。这是正常的，按以下任一方式打开：
   - **右键**（或 Control 点击）**Jasmine.app → 打开 → 打开**，或
   - 第一次被拦后，去 **系统设置 → 隐私与安全性 →「仍要打开」**。
   - 还打不开，就在终端清掉隔离标记：
     ```bash
     xattr -dr com.apple.quarantine /Applications/Jasmine.app
     ```

   这一步只需做一次。

### Windows

> **v0.0.1 暂未包含 Windows 安装器** —— 下一版补上。Windows 用户当前可[从源码构建](#从源码构建)。

1. (上线后)在[发布页](https://github.com/arkyu2077/jasmine/releases/latest)下载 `…_x64-setup.exe` 安装器。
2. 运行它。因为没做代码签名，**SmartScreen** 可能拦截——点 **更多信息 → 仍要运行**。
3. 需要 WebView2（Windows 11 已预装；Windows 10 缺失时安装器会自动获取）。

> **为什么会有安全警告？** Jasmine 是独立开源应用，尚未做 Apple/Microsoft 的付费签名与公证。上面这些是运行你信任的未签名应用的标准做法。你也可以选择[自己从源码构建](#从源码构建)。

---

## 怎么用

1. **打开一个文件夹。** 里面的图片和视频出现在画布上。拖拽、粘贴或「添加图片…」带入更多。这个文件夹**就是** Codex 读写的地方——文件始终在你磁盘上、任何工具都能读。
2. **平移 / 缩放。** 双指滚动平移，⌘/Ctrl + 滚动缩放。点击选中，Shift 拖拽框选，拖动移动；角点缩放，顶部把手旋转。
3. **标记区域**（可选）。按 `R`（或选 ▭ 工具）在图上拖一个框、加备注。`V` 回到选择。标记会画在图上、一起发给 Codex——所以你是「指」，不是「描述」。
4. **问 Codex。** 选中图后输入指令（或点「去背景」之类预设）发送。用 **「+」** 菜单可附加上传、或为这一轮挑一个已启用的 Codex 插件。
5. **结果落在源图右侧。** 继续就好——「再暖一点」「换个背景」「做成 10 秒的片段」。会话一直接着聊，原图永远不动，每个产出都是一个新文件，就放在它的来源旁边。

## 功能

- 一块无限**画布**——平移、缩放、框选、拖动、缩放旋转、小地图、适应全部 / 缩放到选区。
- **区域标记**（矩形 / 椭圆 / 箭头 / 笔刷 / 点），每个标记可加备注，作为标注图一起发给 Codex。
- **预设**一键操作（去背景、变高清…）+ 自由指令。
- **视频与动效**——打包的 `ffmpeg` 做裁剪、拼接、格式转换，外加 HTML→视频渲染；生成的片段直接在画布上**实时播放**。
- **对比**前后滑块或左右并排；**裁切**、复制、在文件管理器中显示、导出。
- **撤销 / 重做**，每个文件夹可开**多条对话**（完整历史都保存），**工作区**侧栏。
- **一条连续的 Codex 对话**：回复实时流出、能看到它的操作步骤、需要时会反问你。
- **设置 + 网络代理**、统一日志、**中英文界面（English / 中文）**、系统托盘。
- 平台:**macOS** —— 原生 Apple Silicon + Intel。**Windows 下一版补上。**

## 工作原理

Jasmine = **Tauri 2（Rust）+ React + PixiJS v8**。Web 层管 chrome，GPU 合成器管画布。图片经自定义 Jasmine 图片协议喂给画布（路径规范化 + 防穿越），Rust 在主线程外负责解码 / 降采样 / mipmap。

Codex 作为长驻 **`codex app-server`** sidecar 运行（JSON-RPC 2.0 over stdio）——每个 board 一个进程，所以会话是真正有状态的。圈选区域会拍平成一张蒙层图，连同干净原图一起以文件路径的形式发给 agent 自读。图像与视频产出会被检测、存成新文件、落在源图右侧，并标明它是从哪张改来的。

---

<a id="从源码构建"></a>

## 从源码构建

面向开发者，或你不想运行未签名的二进制。

**前置：** [Codex CLI](https://developers.openai.com/codex/cli)（已登录）、Node 20+ 与 **pnpm**、**Rust** 工具链（rustup）。macOS 另需 Xcode 命令行工具（`xcode-select --install`）；Windows 需带 *Desktop development with C++* 的 Build Tools + WebView2。

```bash
./setup.sh            # macOS——检查工具链、添加 Rust target、装依赖
.\setup.ps1           # Windows（PowerShell）

pnpm install
pnpm tauri dev        # 启动桌面 app，热重载
```

### 打包

构建脚本在仓库根目录（`.sh` = macOS，`.ps1` = Windows）。Tauri 不能在 macOS 与 Windows 之间交叉编译，所以每个发布构建必须在各自系统上跑。

| | macOS | Windows |
|---|---|---|
| 首次安装 | `./setup.sh` | `.\setup.ps1` |
| 开发包（不签名，快） | `./build_dev.sh` → `Jasmine.app` | `.\build_dev.ps1` → `jasmine.exe` |
| 发布包 | `./build_release.sh` → 分架构 `.dmg` | `.\build_release.ps1` → NSIS 安装器 |
| 发布到 GitHub Releases | `./publish_release.sh` | `.\publish_release.ps1` |

发布签名/公证可选，从 `.env` 读取（macOS）——见 [`.env.example`](./.env.example)。不配则不签名（本机没问题；用户会看到上面说明的 Gatekeeper/SmartScreen 提示）。

## 状态

**v0.0.1 —— 首个公开版本。** 全链路跑通:开文件夹 → 铺到画布 → 指/标记 → 问 Codex → 产出落在它的来源图旁边 → 继续。已发布到 [GitHub Releases](https://github.com/arkyu2077/jasmine/releases)。首个构建为 **macOS** —— 原生 Apple Silicon + Intel;**Windows 安装器下一版补上。**

## 致谢

Jasmine 站在 **[Cameo](https://github.com/hAcKlyc/cameo)** 的肩膀上——是它启发了这个项目。特别感谢原作者们的最初构想，以及 Jasmine 直接据以构建的奠基性工作。秉承这一精神，Jasmine 沿用相同的 **AGPL-3.0-or-later** 协议。

## 许可与声明

采用 **[AGPL-3.0-or-later](./LICENSE)** 许可。你可以在其条款下使用、修改、再分发 Jasmine；若你将修改版作为网络服务运行，AGPL 要求你向其用户提供源码。

Jasmine 是独立的非官方工具。它驱动 Codex CLI，但**不**隶属于 OpenAI，也未获其背书或赞助。「Codex」及相关名称归各自所有者所有。
