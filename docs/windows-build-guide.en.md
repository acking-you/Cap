# Cap Windows Build Guide

This document provides detailed instructions for building the Cap desktop application from source on Windows.

## Prerequisites

Before you begin, ensure the following tools are installed on your system:

- **Node.js 20+** or **Node.js 24+** (recommended)
- **Rust 1.88.0+**
- **pnpm 10.5.2+**
- **winget** (Windows Package Manager, built-in on Windows 10 1809+ or Windows 11)

### Verify Installed Tools

```bash
node --version    # Should output v20.x.x or higher
rustc --version   # Should output rustc 1.88.0 or higher
pnpm --version    # Should output 10.5.2 or higher
winget --version  # Should output version number
```

## Step 1: Clone the Repository

```bash
git clone https://github.com/CapSoftware/Cap.git
cd Cap
```

## Step 2: Install Visual Studio Build Tools

Cap requires Microsoft C++ build tools to compile Rust code.

```bash
winget install Microsoft.VisualStudio.2022.BuildTools --override "--quiet --add Microsoft.VisualStudio.Workload.VCTools --add Microsoft.VisualStudio.Component.VC.Tools.x86.x64 --add Microsoft.VisualStudio.Component.Windows11SDK.22000 --includeRecommended"
```

**Installation time**: Approximately 5-10 minutes, depending on network speed.

**What gets installed**:
- MSVC compiler and linker
- Windows SDK
- C++ build tools

## Step 3: Install LLVM

LLVM provides `libclang.dll`, which is required for Rust's FFI binding generator (bindgen).

```bash
winget install LLVM.LLVM --silent
```

**Installation time**: Approximately 2-3 minutes.

**Explanation**: bindgen needs libclang to parse C/C++ header files and automatically generate Rust bindings for FFmpeg and Whisper.

## Step 4: Install Project Dependencies

```bash
pnpm install
```

This will install all JavaScript/TypeScript dependencies.

**Installation time**: Approximately 2-5 minutes, depending on network speed.

## Step 5: Run Cap Setup Script

```bash
pnpm cap-setup
```

This script will automatically:
1. Download FFmpeg 7.1 (approximately 85MB)
2. Extract FFmpeg to `target/ffmpeg`
3. Copy FFmpeg library files to `target/native-deps`
4. Locate libclang.dll path
5. Generate `.cargo/config.toml` configuration file

**Expected output**:
```
Downloaded ffmpeg-7.1.zip
Extracted ffmpeg
Copied ffmpeg dylibs to target/debug
Copied ffmpeg/lib and ffmpeg/include to target/native-deps
Found LLVM at C:\Program Files\LLVM\bin
```

## Step 6: Configure Environment Variables (.env)

If there's no `.env` file in the project root, create one:

```bash
pnpm env-setup
```

Or manually create a `.env` file with the following minimal configuration:

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

**Note**: This configuration is only needed for building the desktop app and doesn't require an actual database connection.

## Step 7: Build the Desktop Application

### Method 1: Using Build Script (Recommended)

Create `build-desktop.nu` script (if using nushell):

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

Then run:
```bash
nu build-desktop.nu
```

### Method 2: Using Command Line Directly

Open **"x64 Native Tools Command Prompt for VS 2022"** (search in Start menu), then:

```bash
cd C:\path\to\Cap
pnpm tauri:build
```

**Build time**: First build takes approximately 15-30 minutes; subsequent incremental builds take approximately 2-5 minutes.

## Step 8: Run the Application

After the build completes, you can:

1. **Run the development version**:
   ```bash
   ./target/release/Cap - Development.exe
   ```

2. **Install the packaged version**:
   ```bash
   ./target/release/bundle/nsis/Cap - Development_0.3.77_x64-setup.exe
   ```

## Build Artifacts

After a successful build, the following files will be generated:

```
target/release/
├── Cap - Development.exe              # Standalone executable
├── *.dll                              # FFmpeg runtime libraries
└── bundle/
    └── nsis/
        └── Cap - Development_0.3.77_x64-setup.exe  # Windows installer
```

- **Standalone executable**: Must be in the same directory as FFmpeg DLLs
- **Installer**: Contains all dependencies and can be distributed to other users

## Troubleshooting

### 1. libclang.dll not found

**Error message**:
```
Unable to find libclang: "couldn't find any valid shared libraries..."
```

**Solution**:
```bash
winget install LLVM.LLVM --silent
```

Then re-run `pnpm cap-setup`.

### 2. link.exe error

**Error message**:
```
error: linking with `link.exe` failed: exit code: 1
```

**Solution**:
Ensure Visual Studio Build Tools is installed and run the build command in **"x64 Native Tools Command Prompt"**.

### 3. FFmpeg library not found

**Error message**:
```
error: failed to run custom build command for `ffmpeg-sys-next`
```

**Solution**:
```bash
pnpm cap-setup
```

Ensure `.cargo/config.toml` file is generated.

### 4. Missing DLLs at runtime

**Error message**: Windows prompts that files like `avcodec-61.dll` cannot be found.

**Solution**:
This has been fixed in the Tauri configuration. If you encounter this issue, ensure `apps/desktop/src-tauri/tauri.conf.json` contains:

```json
{
  "bundle": {
    "resources": {
      "../../../target/ffmpeg/bin/*.dll": "./"
    }
  }
}
```

Then rebuild.

## Development Mode

If you want to run in development mode (with hot reload):

```bash
pnpm dev:desktop
```

**Note**: Development mode requires Docker to run MySQL and MinIO (S3) services. If you only want to test the desktop app UI, you can skip database-related features.

## Dependencies Explained

### Why are these tools needed?

| Tool | Purpose | Explanation |
|------|---------|-------------|
| **Rust** | Compile Tauri backend | Cap desktop uses Tauri v2 (Rust) |
| **Node.js** | Compile frontend code | SolidStart frontend framework |
| **pnpm** | Package manager | Monorepo dependency management |
| **Visual Studio Build Tools** | C++ linker | Rust on Windows requires MSVC toolchain |
| **LLVM** | libclang.dll | For FFI binding generation (bindgen) |
| **FFmpeg** | Video processing | Screen recording and video encoding |

## Related Links

- [Cap Official Website](https://cap.so)
- [Cap GitHub Repository](https://github.com/CapSoftware/Cap)
- [Tauri Documentation](https://tauri.app)
- [Contributing Guide](../CONTRIBUTING.md)

## Changelog

- **2025-01-25**: Initial version with complete Windows build process
- Fixed tar extraction issue in `scripts/setup.js` for Windows
- Fixed libclang path detection logic
- Added automatic FFmpeg DLL packaging configuration

## License

See the [LICENSE](../LICENSE) file in the project root.
