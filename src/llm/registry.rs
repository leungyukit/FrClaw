//! Provider 工厂与降级链。
//!
//! 当前只落地了 `OpenAiCompatProvider`。其它协议（Anthropic / Wenxin / Zhipu SDK）
//! 留作扩展点。当 `models.yaml` 里标记 `protocol: anthropic` 但没有现成实现时，
//! 这里会退化为 stub 并返回明确错误，提示用户换 base_url 或 fork。

use crate::config::models::{ModelsFile, ProviderConfig};
use crate::error::{Error, Result};
use crate::llm::openai_compat::OpenAiCompatProvider;
use crate::llm::provider::{CompletionRequest, LlmProvider};
use std::sync::Arc;

/// 根据 `models.yaml` 配置 + alias 创建 Provider 实例。
pub fn build_provider(alias: &str, cfg: &ProviderConfig) -> Result<Arc<dyn LlmProvider>> {
    match cfg.protocol.as_str() {
        "openai" => Ok(Arc::new(OpenAiCompatProvider::new(alias, cfg.clone())?)),
        other => Err(Error::Other(format!(
            "暂不支持的协议类型 `{other}`，当前实现: openai。如果你要的不是 OpenAI 兼容，\
             请联系我们在 src/llm/ 里增加 Provider 实现"
        ))),
    }
}

/// 降级链：默认 provider 失败时，自动切到 backup。
pub struct FallbackChain {
    providers: Vec<(String, Arc<dyn LlmProvider>)>,
}

impl FallbackChain {
    pub fn from_models(models: &ModelsFile) -> Result<Self> {
        let mut providers = Vec::new();
        if let Some(name) = models.default_provider_name() {
            if let Some(cfg) = models.get(&name) {
                match build_provider(&name, cfg) {
                    Ok(p) => providers.push((name, p)),
                    Err(_) => {} // 配置问题不阻塞启动
                }
            }
        }
        if let Some(name) = models.backup_provider_name() {
            if providers.iter().all(|(n, _)| n != &name) {
                if let Some(cfg) = models.get(&name) {
                    if let Ok(p) = build_provider(&name, cfg) {
                        providers.push((name, p));
                    }
                }
            }
        }
        Ok(Self { providers })
    }

    pub fn primary(&self) -> Option<&Arc<dyn LlmProvider>> {
        self.providers.first().map(|(_, p)| p)
    }

    pub fn primary_alias(&self) -> Option<&str> {
        self.providers.first().map(|(n, _)| n.as_str())
    }

    pub fn providers(&self) -> &[(String, Arc<dyn LlmProvider>)] {
        &self.providers
    }

    /// 按 alias 查 provider，找不到返回 None。
    pub fn find(&self, alias: &str) -> Option<&Arc<dyn LlmProvider>> {
        self.providers
            .iter()
            .find(|(n, _)| n == alias)
            .map(|(_, p)| p)
    }

    /// 兼容旧 API：pick the alias 给定的，没有就 primary。
    pub fn primary_for_alias(&self, alias: &str) -> Option<&Arc<dyn LlmProvider>> {
        self.find(alias).or_else(|| self.primary())
    }

    /// 按 preferred alias 跑 chat，失败时按链顺序退到 backup。
    ///
    /// - `preferred_alias`：希望优先使用的 provider（`session.provider_alias`）
    /// - 链里不存在该 alias 时 fall through 到链顺序
    pub async fn chat_with_fallback(
        &self,
        preferred_alias: &str,
        req: CompletionRequest,
    ) -> Result<(String, crate::llm::provider::CompletionResponse)> {
        let mut ordered: Vec<(String, Arc<dyn LlmProvider>)> = Vec::new();
        if let Some((_, p)) = self.providers.iter().find(|(n, _)| n == preferred_alias) {
            ordered.push((preferred_alias.to_string(), Arc::clone(p)));
        }
        for (alias, p) in &self.providers {
            if alias != preferred_alias {
                ordered.push((alias.clone(), Arc::clone(p)));
            }
        }

        let mut last_err: Option<anyhow::Error> = None;
        for (alias, p) in ordered {
            match p.chat(req.clone()).await {
                Ok(r) => return Ok((alias, r)),
                Err(e) => last_err = Some(e),
            }
        }
        match last_err {
            Some(e) => Err(e.into()),
            None => Err(Error::Other("降级链为空".into()).into()),
        }
    }
}
