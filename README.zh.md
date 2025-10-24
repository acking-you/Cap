<p align="center">
  <p align="center">
   <img width="150" height="150" src="https://github.com/CapSoftware/Cap/blob/main/apps/desktop/src-tauri/icons/Square310x310Logo.png" alt="Logo">
  </p>
	<h1 align="center"><b>Cap</b></h1>
	<p align="center">
		开源的 Loom 替代方案。
    <br />
    <a href="https://cap.so"><strong>Cap.so »</strong></a>
    <br />
    <br />
    <b>下载地址：</b>
		<a href="https://cap.so/download">macOS & Windows</a>
    <br />
    <br />
    <a href="README.md">English</a> · <a href="README.zh.md">简体中文</a>
  </p>
</p>
<br/>

[![Open Bounties](https://img.shields.io/endpoint?url=https%3A%2F%2Fconsole.algora.io%2Fapi%2Fshields%2FCapSoftware%2Fbounties%3Fstatus%3Dopen)](https://console.algora.io/org/CapSoftware/bounties?status=open)

Cap 是 Loom 的开源替代方案。它是一个视频消息工具，让你能在几秒钟内录制、编辑和分享视频。

<img src="https://raw.githubusercontent.com/CapSoftware/Cap/refs/heads/main/apps/web/public/landing-cover.png"/>

# 自托管部署

Cap Web 可以使用 Docker 或 Railway 进行自托管，详情请参阅我们的[自托管文档](https://cap.so/docs/self-hosting)。
你也可以使用下面的按钮将 Cap Web 部署到 Railway：

[![Deploy on Railway](https://railway.com/button.svg)](https://railway.com/new/template/PwpGcf)

无论你是自己构建还是从我们的[官网下载](https://cap.so/download)，Cap Desktop 都可以连接到你自托管的 Cap Web 实例。

# 从源码构建

## Windows

详细的构建说明请参阅 [Windows 构建指南](docs/windows-build-guide.zh.md)。

# Monorepo 应用架构

我们在这个由 Turborepo 驱动的 monorepo 中使用了 Rust、React (Next.js)、TypeScript、Tauri、Drizzle (ORM)、MySQL、TailwindCSS 等技术栈。

> 关于数据库的说明：代码库目前仅设计用于 MySQL。MariaDB 或其他兼容数据库可能部分工作，但不受官方支持。

### 应用程序：

- `desktop`：一个 [Tauri](https://tauri.app) (Rust) 应用，前端使用 [SolidStart](https://start.solidjs.com)。
- `web`：一个 [Next.js](https://nextjs.org) Web 应用。

### 包：

- `ui`：一个 [React](https://reactjs.org) 共享组件库。
- `utils`：一个 [React](https://reactjs.org) 共享工具库。
- `tsconfig`：整个 monorepo 中使用的共享 `tsconfig` 配置。
- `database`：一个 [React](https://reactjs.org) 和 [Drizzle ORM](https://orm.drizzle.team/) 共享数据库库。
- `config`：`eslint` 配置（包括 `eslint-config-next`、`eslint-config-prettier` 以及整个 monorepo 中使用的其他配置）。

### 许可证：

本软件的部分内容按如下方式授权：

- 所有位于 `cap-camera*` 和 `scap-*` crate 系列中的代码均采用 MIT 许可证（参见 [licenses/LICENSE-MIT](https://github.com/CapSoftware/Cap/blob/main/licenses/LICENSE-MIT)）。
- 所有第三方组件均按其所有者提供的原始许可证授权
- 上述未提及的所有其他内容均在 [LICENSE](https://github.com/CapSoftware/Cap/blob/main/LICENSE) 中定义的 AGPLv3 许可证下可用

# 贡献

更多信息请参见 [CONTRIBUTING.md](CONTRIBUTING.md)。本指南正在不断完善中，随着应用的成熟会定期更新。
