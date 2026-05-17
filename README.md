# 🤖 My Code Agent

基于 DeepSeek 的 AI 编程助手，支持在终端中读取、写入、搜索和执行代码。

## 核心功能

- **交互式对话** — 多轮对话与流式响应
- **工具增强** — 文件操作、代码搜索、Shell 命令执行
- **多智能体协作** — 并行执行专业子智能体（代码审查、安全分析等）
- **会话持久化** — 可选保存和加载对话会话

## 快速开始

### 前置要求
- [Rust](https://www.rust-lang.org/tools/install) (2024+)
- [DeepSeek API 密钥](https://platform.deepseek.com/)

### 安装与运行
```bash
# 从源码构建
git clone <repo-url>
cd my-code-agent
cargo build --release

# 配置 API 密钥
echo "DEEPSEEK_API_KEY=your_api_key_here" > .env

# 运行
./target/release/my-code-agent
```

## 基本使用

启动后输入消息开始对话：
```
❯ 读取 src/main.rs 并解释其工作原理
❯ 搜索所有使用 TokenUsage 结构体的地方
❯ 运行 cargo test 并显示结果
```

使用 `@` 附加文件：
```
❯ 解释 @src/main.rs
❯ 比较 @src/main.rs 和 @src/lib.rs
```

## 配置

创建 `config.toml` 文件（可选）：
```toml
[llm]
provider = "deepseek"
model = "deepseek-reasoner"
api_key_env = "DEEPSEEK_API_KEY"

[context]
window_size = 1048576

[shell]
default_timeout_secs = 30
```

## 许可证
MIT