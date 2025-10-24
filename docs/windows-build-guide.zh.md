# Cap Windows 编译指南

本文档详细描述如何在 Windows 平台上从源码编译 Cap 桌面应用程序。

## 前置要求

在开始之前，请确保你的系统已安装以下工具：

- **Node.js 20+** 或 **Node.js 24+**（推荐）
- **Rust 1.88.0+**
- **pnpm 10.5.2+**
- **winget**（Windows 包管理器，Windows 10 1809+ 或 Windows 11 自带）

### 验证已安装工具

```bash
node --version    # 应输出 v20.x.x 或更高
rustc --version   # 应输出 rustc 1.88.0 或更高
pnpm --version    # 应输出 10.5.2 或更高
winget --version  # 应输出版本号
```

## 步骤 1：克隆仓库

```bash
git clone https://github.com/CapSoftware/Cap.git
cd Cap
```

## 步骤 2：安装 Visual Studio Build Tools

Cap 需要 Microsoft C++ 构建工具来编译 Rust 代码。

```bash
winget install Microsoft.VisualStudio.2022.BuildTools --override "--quiet --add Microsoft.VisualStudio.Workload.VCTools --add Microsoft.VisualStudio.Component.VC.Tools.x86.x64 --add Microsoft.VisualStudio.Component.Windows11SDK.22000 --includeRecommended"
```

**安装时间**：约 5-10 分钟，根据网络速度而定。

**安装内容**：
- MSVC 编译器和链接器
- Windows SDK
- C++ 构建工具

## 步骤 3：安装 LLVM

LLVM 提供 `libclang.dll`，用于 Rust 的 FFI 绑定生成器（bindgen）。

```bash
winget install LLVM.LLVM --silent
```

**安装时间**：约 2-3 分钟。

**说明**：bindgen 需要 libclang 来解析 C/C++ 头文件，以自动生成 FFmpeg 和 Whisper 的 Rust 绑定。

## 步骤 4：安装项目依赖

```bash
pnpm install
```

这将安装所有 JavaScript/TypeScript 依赖。

**安装时间**：约 2-5 分钟，根据网络速度而定。

## 步骤 5：运行 Cap 设置脚本

```bash
pnpm cap-setup
```

这个脚本会自动：
1. 下载 FFmpeg 7.1（约 85MB）
2. 解压 FFmpeg 到 `target/ffmpeg`
3. 复制 FFmpeg 库文件到 `target/native-deps`
4. 查找 libclang.dll 路径
5. 生成 `.cargo/config.toml` 配置文件

**预期输出**：
```
Downloaded ffmpeg-7.1.zip
Extracted ffmpeg
Copied ffmpeg dylibs to target/debug
Copied ffmpeg/lib and ffmpeg/include to target/native-deps
Found LLVM at C:\Program Files\LLVM\bin
```

## 步骤 6：配置环境变量（.env）

如果项目根目录没有 `.env` 文件，创建一个：

```bash
pnpm env-setup
```

或手动创建 `.env` 文件，包含以下最小配置：

```env
# .env
NEXT_PUBLIC_WEB_URL=http://localhost:3000
NODE_ENV=development
WEB_URL=http://localhost:3000
DATABASE_URL=mysql://root:root@localhost:3306/cap
NEXTAUTH_SECRET=development-secret-key-please-change-in-production
NEXTAUTH_URL=http://localhost:3000
CAP_AWS_BUCKET=cap-local
CAP_AWS_REGION=us-east-1
S3_PATH_STYLE=true
WORKFLOWS_RPC_SECRET=development-rpc-secret
```

**注意**：这个配置仅用于构建桌面应用，不需要实际的数据库连接。

## 步骤 7：构建桌面应用

### 方法 1：使用构建脚本（推荐）

创建 `build-desktop.nu` 脚本（如果使用 nushell）：

```nu
#!/usr/bin/env nu

# Build Cap Desktop Application for Windows

print "🚀 Starting Cap Desktop build..."

let bat_content = '@echo off
call "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat"
pnpm tauri:build
'

print "📝 Creating temporary build script..."
$bat_content | save -f temp-build.bat

print "🔨 Running build with Visual Studio environment..."
cmd /c temp-build.bat

print "🧹 Cleaning up..."
rm temp-build.bat

print "✅ Build completed!"
print "📦 Executable at: target/release/Cap - Development.exe"
print "📦 Installer at: target/release/bundle/nsis/Cap - Development_0.3.77_x64-setup.exe"
```

然后运行：
```bash
nu build-desktop.nu
```

### 方法 2：直接使用命令行

打开 **"x64 Native Tools Command Prompt for VS 2022"**（在开始菜单搜索），然后：

```bash
cd C:\path\to\Cap
pnpm tauri:build
```

**构建时间**：首次构建约 15-30 分钟，后续增量构建约 2-5 分钟。

## 步骤 8：运行应用

构建完成后，你可以：

1. **运行开发版本**：
   ```bash
   ./target/release/Cap - Development.exe
   ```

2. **安装打包版本**：
   ```bash
   ./target/release/bundle/nsis/Cap - Development_0.3.77_x64-setup.exe
   ```

## 构建产物说明

成功构建后，会生成以下文件：

```
target/release/
├── Cap - Development.exe              # 独立可执行文件
├── *.dll                              # FFmpeg 运行时库
└── bundle/
    └── nsis/
        └── Cap - Development_0.3.77_x64-setup.exe  # Windows 安装程序
```

- **独立可执行文件**：需要与 FFmpeg DLL 在同一目录
- **安装程序**：包含所有依赖，可以分发给其他用户

## 常见问题

### 1. 找不到 libclang.dll

**错误信息**：
```
Unable to find libclang: "couldn't find any valid shared libraries..."
```

**解决方案**：
```bash
winget install LLVM.LLVM --silent
```

然后重新运行 `pnpm cap-setup`。

### 2. link.exe 错误

**错误信息**：
```
error: linking with `link.exe` failed: exit code: 1
```

**解决方案**：
确保已安装 Visual Studio Build Tools，并在 **"x64 Native Tools Command Prompt"** 中运行构建命令。

### 3. FFmpeg 库找不到

**错误信息**：
```
error: failed to run custom build command for `ffmpeg-sys-next`
```

**解决方案**：
```bash
pnpm cap-setup
```

确保 `.cargo/config.toml` 文件已生成。

### 4. 运行时缺少 DLL

**错误信息**：Windows 提示找不到 `avcodec-61.dll` 等文件。

**解决方案**：
这已经在 Tauri 配置中修复。如果遇到此问题，请确保 `apps/desktop/src-tauri/tauri.conf.json` 中包含：

```json
{
  "bundle": {
    "resources": {
      "../../../target/ffmpeg/bin/*.dll": "./"
    }
  }
}
```

然后重新构建。

## 开发模式

如果你想运行开发模式（热重载）：

```bash
pnpm dev:desktop
```

**注意**：开发模式需要 Docker 来运行 MySQL 和 MinIO（S3）服务。如果只想测试桌面应用的 UI，可以跳过数据库相关功能。

## 依赖说明

### 为什么需要这些工具？

| 工具 | 用途 | 说明 |
|------|------|------|
| **Rust** | 编译 Tauri 后端 | Cap 桌面端使用 Tauri v2（Rust） |
| **Node.js** | 编译前端代码 | SolidStart 前端框架 |
| **pnpm** | 包管理器 | Monorepo 依赖管理 |
| **Visual Studio Build Tools** | C++ 链接器 | Rust 在 Windows 上需要 MSVC 工具链 |
| **LLVM** | libclang.dll | 用于 FFI 绑定生成（bindgen） |
| **FFmpeg** | 视频处理 | 屏幕录制和视频编码 |

## 相关链接

- [Cap 官方网站](https://cap.so)
- [Cap GitHub 仓库](https://github.com/CapSoftware/Cap)
- [Tauri 文档](https://tauri.app)
- [贡献指南](../CONTRIBUTING.md)

## 更新日志

- **2025-01-25**：初始版本，包含完整的 Windows 编译流程
- 修复了 `scripts/setup.js` 中 Windows 下的 tar 解压问题
- 修复了 libclang 路径查找逻辑
- 添加了 FFmpeg DLL 自动打包配置

## 许可证

参见项目根目录的 [LICENSE](../LICENSE) 文件。
