//! R07-T07 β4：日记面（现役 server/routes/diary.ts + lib/diary/
//! diary-writer.ts）真实 loopback 集成测试；模型面全部走确定性 TCP
//! 替身（dream 夹具模式），不打真实供应商。
#![allow(clippy::too_many_lines)]

#[path = "r07_diary/mod.rs"]
mod r07_diary;
