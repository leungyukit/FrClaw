//! REPL 共享上下文。
//!
//! 整个会话期间不变量：models 配置、settings、ChatSession、provider 链，
//! 以及 agent 状态（授权、Plan mode、SubAgent 仓库、思维模式、自我记忆进化的 auto timer、
//! Round 4 hooks 配置、Round 5 skills 注册表）。

use crate::agent::ThinkingMode;
use crate::config::models::ModelsFile;
use crate::config::settings::Settings;
use crate::heartbeat::HeartbeatRegistry;
use crate::hermes::HermesRegistry;
use crate::hooks::HooksFile;
use crate::llm::registry::FallbackChain;
use crate::mcp::McpManager;
use crate::memory::evolution::EvolutionAuto;
use crate::memory::project::ProjectMemory;
use crate::rag::RagStore;
use crate::sandbox::policy::SandboxPolicy;
use crate::session::chat::ChatSession;
use crate::skills::SkillRegistry;
use crate::soul::loader::SoulContent;
use crate::tools::permission::PermissionGate;
use crate::tools::plan::PlanModeState;
use crate::tools::rag::RagContext;
use crate::tools::subagent::SubAgentRegistry;
use crate::tools::worktree::WorktreeContext;
use crate::tools::HeartbeatToolContext;
use std::sync::{Arc, Mutex, RwLock};

pub struct AppContext {
    pub models: Arc<Mutex<ModelsFile>>,
    pub settings: Arc<Mutex<Settings>>,
    pub session: Arc<Mutex<ChatSession>>,
    pub chain: Arc<RwLock<FallbackChain>>,
    pub cwd: Arc<Mutex<std::path::PathBuf>>,

    pub permission: Arc<Mutex<PermissionGate>>,
    pub plan_state: Arc<Mutex<PlanModeState>>,
    pub sub_agents: Arc<SubAgentRegistry>,
    pub thinking: Arc<Mutex<ThinkingMode>>,
    pub project_memory: Arc<Mutex<Option<ProjectMemory>>>,

    pub evolution_auto: Arc<Mutex<Option<EvolutionAuto>>>,

    pub hooks: Arc<Mutex<HooksFile>>,

    /// Round 5 ─ Skills 注册表（启动期 discover；hot-reload 用 `reload()`）。
    pub skills: Arc<SkillRegistry>,

    /// Round 6 ─ MCP Manager（Streamable HTTP 客户端 + 工具发现）。
    pub mcp: McpManager,

    /// Round 7 ─ RAG 个人知识库（sqlite + hash embedder，可切 provider）。
    pub rag: Arc<RagContext>,

    /// Round 8 ─ Git worktree 上下文（启动期 discover git root）。
    pub worktree: Arc<WorktreeContext>,

    /// Round 9 ─ 沙箱策略（路径/命令检查；macOS 可选 sandbox-exec 增强）。
    pub sandbox: Arc<Mutex<SandboxPolicy>>,

    /// Round 12 ─ Hermes 后台任务引擎。
    pub hermes: HermesRegistry,

    /// Round 13 ─ SOUL.md 持久身份（多源合并 + 工具）。
    pub soul: Arc<crate::tools::soul::SoulContext>,

    /// Round 14 ─ Heartbeat 主动唤醒。
    pub heartbeat: HeartbeatRegistry,

    /// Round 14 ─ Heartbeat 工具上下文（LLM 可见的 heartbeat_status/now/set）。
    pub heartbeat_tools: Arc<HeartbeatToolContext>,

    /// Round 16 ─ 多通讯通道（飞书 / 钉钉 / 企微 / 通用 Webhook）
    pub channels: crate::channels::ChannelManager,
}

impl AppContext {
    pub fn new(
        models: ModelsFile,
        settings: Settings,
        session: ChatSession,
        chain: FallbackChain,
        cwd: std::path::PathBuf,
    ) -> Self {
        let autonomous = settings.autonomous;
        let skills = SkillRegistry::discover().unwrap_or_default();
        let mcp = McpManager::from_config();
        let rag = Arc::new(RagContext::new(
            RagStore::open(crate::config::paths::rag_db_path())
                .unwrap_or_else(|_| RagStore::open_in_memory().expect("in-mem rag fallback")),
        ));
        let worktree = Arc::new(WorktreeContext::discover(&cwd));
        let sandbox = Arc::new(Mutex::new(SandboxPolicy::load_or_default()));
        // Round 12 ─ Hermes 任务引擎 + 后台 tick
        let hermes = crate::hermes::registry::start();
        // Round 13 ─ SOUL.md 多源合并
        let soul = Arc::new(crate::tools::soul::SoulContext::new(
            SoulContent::load(&cwd),
            cwd.clone(),
        ));
        // Round 14 ─ Heartbeat 状态加载
        let heartbeat = HeartbeatRegistry::new();
        let heartbeat_tools = Arc::new(HeartbeatToolContext::new(soul.clone()));
        *heartbeat_tools.app_ctx_factory.lock().unwrap() = None; // 后面 bootstrap 注入
        // 启动期 reload policy
        heartbeat_tools.reload_policy();
        // state.enabled 跟 policy.enabled 同步
        {
            let pol_enabled = heartbeat_tools.policy.lock().unwrap().enabled;
            let mut st = heartbeat_tools.state.lock().unwrap();
            st.enabled = pol_enabled;
        }
        Self {
            models: Arc::new(Mutex::new(models)),
            settings: Arc::new(Mutex::new(settings)),
            session: Arc::new(Mutex::new(session)),
            chain: Arc::new(RwLock::new(chain)),
            cwd: Arc::new(Mutex::new(cwd)),
            permission: Arc::new(Mutex::new(PermissionGate::new(autonomous))),
            plan_state: Arc::new(Mutex::new(PlanModeState::default())),
            sub_agents: Arc::new(SubAgentRegistry::new()),
            thinking: Arc::new(Mutex::new(ThinkingMode::ReAct)),
            project_memory: Arc::new(Mutex::new(None)),
            evolution_auto: Arc::new(Mutex::new(None)),
            hooks: Arc::new(Mutex::new(HooksFile::load_or_default())),
            skills: Arc::new(skills),
            mcp,
            rag,
            worktree,
            sandbox,
            hermes,
            soul,
            heartbeat,
            heartbeat_tools,
            channels: crate::channels::ChannelManager::from_file(),
        }
    }
}
