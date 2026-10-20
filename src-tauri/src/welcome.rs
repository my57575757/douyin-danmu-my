//! 欢迎消息：进房 / 关注 / 点赞的限流与去重
//!
//! 这类事件的量级比弹幕大（抖音实测进房 : 弹幕 ≈ 2 : 1，房间 765790908625 的 120 秒里
//! 224 条进场 / 114 条弹幕），原样广播会把弹幕窗的 120 条上限吃光、真弹幕全被顶掉，
//! 所以先过两道闸：
//!
//! 1. 去重：同一个人在 `DEDUPE_SECS` 内只出一条（离开再进、连点、多标签页都会刷）
//! 2. 限速：每 `RATE_WINDOW` 最多 `MAX_PER_WINDOW` 条，**所有形态共用一个池**
//!    （分形态各一份的话合起来就是二十几条/分钟，比中位房间的弹幕还密，那就是刷屏）
//!
//! 丢掉的欢迎消息不补发——它是「热闹感」，不是必须送达的消息。
//!
//! 抖音没有舰队（舰长进场），所以 B 站侧那套「舰长进场单独一个小池」的额度
//! 在这里不存在；朗读侧也不再消费欢迎事件（没有值得单独出声的档位）。

use crate::douyin::event::{Welcome, WelcomeKind};
use crate::state::OverlayState;
use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

/// 同一人同类事件的最小间隔（秒）
const DEDUPE_SECS: u64 = 60;
/// 限速窗口
const RATE_WINDOW: Duration = Duration::from_secs(30);
/// 每个限速窗口内最多放行条数（≈ 2 条/分钟）
const MAX_PER_WINDOW: usize = 1;
/// 去重表容量上限
const DEDUPE_CAP: usize = 4096;

/// 欢迎消息运行态（Tauri 托管）
#[derive(Default)]
pub(crate) struct WelcomeState {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    /// (形态, uid) → 上次放行时间
    seen: HashMap<(WelcomeKind, i64), Instant>,
    /// 所有形态共用的额度
    slots: VecDeque<Instant>,
}

impl Inner {
    /// 判定这条欢迎消息是否放行
    fn admit(&mut self, w: &Welcome, now: Instant) -> bool {
        // 1. 去重：同一个人同类事件在窗口内只出一条
        let key = (w.kind, w.uid);
        if let Some(last) = self.seen.get(&key) {
            if now.duration_since(*last) < Duration::from_secs(DEDUPE_SECS) {
                return false;
            }
        }
        // 2. 限速：所有形态共用一个池
        while self
            .slots
            .front()
            .is_some_and(|t| now.duration_since(*t) >= RATE_WINDOW)
        {
            self.slots.pop_front();
        }
        if self.slots.len() >= MAX_PER_WINDOW {
            return false;
        }
        self.slots.push_back(now);
        // 去重记录只值几十秒，满了整体清空即可，不值得为它做 LRU
        if self.seen.len() >= DEDUPE_CAP {
            self.seen.clear();
        }
        self.seen.insert(key, now);
        true
    }
}

/// 处理一条欢迎消息 → 去重限速 → 广播给弹幕窗
///
/// 去重与限速**与开关无关地先跑**：开关只决定「播不播」。若让开关去挡限速，
/// 关掉显示时同一个观众反复进出就能绕过去重表，开回来后一次刷出一大片。
pub(crate) fn on_welcome(app: &AppHandle, w: Welcome) {
    // 锁内只判定不广播：emit 是跨 webview 的分发，压在状态锁里迟早把锁序搅乱
    let pass = {
        let st = app.state::<WelcomeState>();
        let mut inner = st.inner.lock().unwrap();
        inner.admit(&w, Instant::now())
    };
    if !pass {
        return;
    }
    // 语音播报与显示各自独立：关着显示也能只听进场欢迎（开关在 tts 侧判定）
    crate::tts::on_welcome(app, &w);
    let render = app.state::<OverlayState>().welcome.lock().unwrap().enabled;
    if render {
        let _ = app.emit("welcome", &w);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn welcome(kind: WelcomeKind, uid: i64) -> Welcome {
        Welcome {
            id: format!("t-{uid}"),
            kind,
            uid,
            username: "观众A".into(),
            timestamp: 1700000300,
        }
    }

    #[test]
    fn 同一人去重窗口内只过一条() {
        let mut inner = Inner::default();
        let now = Instant::now();
        assert!(inner.admit(&welcome(WelcomeKind::Enter, 1), now));
        // 31 秒后：已出限速窗口，但仍在 60 秒去重窗口内 → 挡在去重这一关
        assert!(!inner.admit(&welcome(WelcomeKind::Enter, 1), now + Duration::from_secs(31)));
        // 61 秒后：两个窗口都过期
        assert!(inner.admit(&welcome(WelcomeKind::Enter, 1), now + Duration::from_secs(61)));
    }

    #[test]
    fn 全局额度用完后丢弃() {
        let mut inner = Inner::default();
        let now = Instant::now();
        assert!(inner.admit(&welcome(WelcomeKind::Enter, 1), now));
        assert!(
            !inner.admit(&welcome(WelcomeKind::Enter, 2), now + Duration::from_secs(1)),
            "额度内第二个人也要丢"
        );
        assert!(
            inner.admit(&welcome(WelcomeKind::Enter, 3), now + Duration::from_secs(31)),
            "出窗口后放行"
        );
    }

    #[test]
    fn 不同形态共用同一份全局额度() {
        let mut inner = Inner::default();
        let now = Instant::now();
        assert!(inner.admit(&welcome(WelcomeKind::Enter, 1), now));
        assert!(
            !inner.admit(&welcome(WelcomeKind::Follow, 2), now),
            "换形态不额外给额度——这正是分形态限速会刷屏的原因"
        );
        assert!(!inner.admit(&welcome(WelcomeKind::Like, 3), now));
    }

    #[test]
    fn 高频灌入十分钟只放行二十条() {
        let mut inner = Inner::default();
        let now = Instant::now();
        // 模拟热度房：每 0.5 秒一条进房（实测热房间 80 条/分钟），持续 10 分钟，用户各不相同
        let mut admitted = 0usize;
        for i in 0..1200u64 {
            let w = welcome(WelcomeKind::Enter, i as i64);
            if inner.admit(&w, now + Duration::from_millis(i * 500)) {
                admitted += 1;
            }
        }
        assert_eq!(admitted, 20, "10 分钟 ÷ 30 秒窗口 = 20 条，与灌入速率无关");
    }

    #[test]
    fn 去重表满后整体清空() {
        let mut inner = Inner::default();
        let now = Instant::now();
        for uid in 0..=(DEDUPE_CAP as i64) {
            // 每 31 秒一条，不然会被 30 秒额度挡住，去重表就长不起来
            inner.admit(
                &welcome(WelcomeKind::Enter, uid),
                now + Duration::from_secs(uid as u64 * 31),
            );
        }
        assert!(inner.seen.len() <= DEDUPE_CAP, "清空后表长应回落到容量以内");
    }
}
