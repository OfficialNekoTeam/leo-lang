//! LeoLang 编译器库
//! 
//! # 模块结构
//! 
//! - `common`: 通用工具（错误、源码位置）
//! - `lexer`: 词法分析
//! - `parser`: 语法分析  
//! - `ast`: 抽象语法树
//! - `sema`: 语义分析
//! - `lint`: 五层 lint 检查
//! - `codegen`: 代码生成
//! - `llvm`: LLVM 接口封装
//! - `compiler`: 编译流程

pub mod common;
pub mod lexer;
pub mod parser;
pub mod ast;
pub mod sema;
pub mod lint;
pub mod codegen;
pub mod llvm;
pub mod compiler;