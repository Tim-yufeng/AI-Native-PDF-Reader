import { invoke, Channel } from '@tauri-apps/api/core';
import type { ContextPlan, ContextSource, Region } from './ai-context';
import type { DesktopDescriptor } from './desktop-source';
export interface ModelConfig { provider: 'mock' | 'deepseek' | 'openrouter' | 'compatible'; endpoint: string; model: string; vision: boolean; }
export interface AiConfig { schemaVersion: number; main: ModelConfig; auxiliary: ModelConfig | null; }
// Which configured model answers one request. The choice is explicit and is
// recorded with the answer; it is never a silent fallback.
export type AiSlot = 'main' | 'auxiliary';
export interface AiModelChoice { slot: AiSlot; label: string; detail: string; vision: boolean; configured: boolean; }
export const modelName = (m: ModelConfig): string => m.provider === 'mock' ? 'Mock · 本地测试' : `${m.provider} · ${m.model}`;
export function availableModels(config: AiConfig | null): AiModelChoice[] {
  if (!config) return [{ slot: 'main', label: '主模型', detail: '模型配置读取中…', vision: true, configured: true }];
  const main = modelName(config.main);
  const choices: AiModelChoice[] = [{ slot: 'main', label: '主模型', detail: main, vision: config.main.vision, configured: true }];
  // The auxiliary entry exists only once it is configured, so a selection can
  // never name a slot the backend has no model for.
  if (config.auxiliary) choices.push({ slot: 'auxiliary', label: '辅助模型', detail: modelName(config.auxiliary), vision: config.auxiliary.vision, configured: true });
  return choices;
}
export interface AiEvent { requestId: string; bindingId: string; kind: string; userId?: string; answerId?: string; text?: string; code?: string; usage?: Record<string, unknown>; }
export interface AiDetails { sources?: ContextSource[] | null; usage?: Record<string, unknown> | null; }
export const defaultModel = (): ModelConfig => ({ provider: 'mock', endpoint: '', model: 'mock-vision', vision: true });
export function providerDefaults(provider: ModelConfig['provider']): ModelConfig {
  return provider === 'deepseek' ? { provider, endpoint: 'https://api.deepseek.com/chat/completions', model: 'deepseek-flash', vision: true }
    : provider === 'openrouter' ? { provider, endpoint: 'https://openrouter.ai/api/v1/chat/completions', model: '', vision: false }
    : provider === 'compatible' ? { provider, endpoint: '', model: '', vision: false } : defaultModel();
}
// Every refusal the backend can return is named. A code with no entry here
// would be reported as an unexplained failure, which is exactly what made the
// in-place explanation look like a mysterious "check your settings" error, so
// the mapping is kept exhaustive for the codes ai_network and the AI commands
// actually produce.
const AI_ERRORS: Record<string, string> = {
  authentication: 'API 鉴权失败，请检查安全凭据。', quota: '余额或配额不足。', timeout: '请求超时，可重试。',
  vision: '所选模型未启用图像能力，而本轮包含截图或图像；请改用支持图像的模型，或移除图像后重试。',
  key_missing: '此模型还没有安全凭据，请在模型设置中保存 API Key。',
  key_invalid: '已保存的凭据格式不正确，请重新输入。',
  budget: '相关来源或图像超过本轮容量，未静默省略；请缩小问题范围、页数或区域后重试。',
  // Raised only when the user's own selection is too large. Retrieval is trimmed
  // to fit and can never cause this.
  budget_selection: '你本轮指定的来源本身超出容量（所选页数或文字量过多）；请减少所选页数，或改用区域截图提问。自动检索的相关内容不会挤掉你选的内容。',
  endpoint: 'Endpoint 未通过公网 HTTPS 安全检查。',
  protocol: 'Provider 返回的响应不完整或不受支持。',
  provider_error: 'Provider 报告了生成错误，可重试。',
  invalid_request: '本轮请求不合法，请调整问题或所选内容后重试。',
  invalid_image: '有图像来源没有取到内容（例如没有可提取文字、需要作为图像发送的页面）。请重试；若持续出现，请缩小区域或换一页。',
  index: '本地索引未能建立，请重试。',
  index_incomplete: '本地索引尚未完成，请稍候重试。',
  auxiliary_missing: '尚未配置辅助模型，请在模型设置中填写后再切换；主模型仍可直接使用。',
  source_revoked: '来源授权已撤销或内容已变化，请重新允许同目录检索。',
  busy: '上一轮请求仍在进行，请稍候。',
  denied: '此操作未被允许。',
  io: '本地文件或数据操作未完成，请检查数据目录权限后重试。',
  cancelled: '生成已取消。',
  mock_only: '当前构建仅允许 Mock，真实凭据与网络已禁用。',
  chat_changed: '对话已切换，本轮请求已作废。',
};
export const aiError = (error: unknown): string => {
  const code = error && typeof error === 'object' && 'code' in error ? String(error.code) : '';
  if (code && AI_ERRORS[code]) return AI_ERRORS[code];
  // An AppError carries a reviewed message of its own; prefer it over a
  // generic sentence so the user learns the actual reason.
  const message = error && typeof error === 'object' && 'message' in error && typeof error.message === 'string' ? error.message.trim() : '';
  if (message) return message;
  if (error instanceof Error && error.message) return error.message;
  return code ? `操作未完成（${code}）。` : '操作未完成，请检查模型设置与本地数据状态。';
};
export const aiClient = {
  config: () => invoke<{ config: AiConfig; mainKeyPresent: boolean; auxiliaryKeyPresent: boolean; realProviderEnabled: boolean }>('m5_config'),
  configure: (config: AiConfig) => invoke('m5_set_config', { config }),
  key: (slot: string, key: string, endpoint: string) => invoke('m5_store_key', { slot, key, endpoint }),
  deleteKey: (slot: string) => invoke<boolean>('m5_delete_key', { slot }),
  test: (slot: string) => invoke<{ connected: boolean; vision: boolean; mock?: boolean }>('m5_test_connection', { slot }),
  index: (bindingId: string, pages: { page: number; text: string }[]) => invoke<void>('m5_index', { bindingId, pages }),
  prepare: (bindingId: string, query: string, page: number, selected: number[], currentPdf: boolean, region: Region | null, pinned: number[] = [], parentId: string | null = null, editId: string | null = null, externalImages: { name: string; png: string }[] = [], answerLimit: number | null = null, requireComplete = false) => invoke<ContextPlan>('m5_prepare', { bindingId, input: { query, answerLimit, page: region?.page ?? page, selected, currentPdf, rect: region?.rect ?? null, pinned, parentId, editId, externalImages, requireComplete } }),
  refine: (bindingId: string, preparedId: string, sourceIds: string[]) => invoke<ContextPlan>('m5_refine', {bindingId, preparedId, sourceIds}),
  images: (bindingId: string, preparedId: string, images: { sourceId: string; png: string }[]) => invoke('m5_images', { bindingId, preparedId, images }),
  start: (bindingId: string, preparedId: string, parentId: string | null, editId: string | null, event: (event: AiEvent) => void, slot: AiSlot = 'main') => {
    const onEvent = new Channel<AiEvent>(); onEvent.onmessage = event;
    return invoke<string>('m5_start', { input: { bindingId, preparedId, parentId, editId, slot }, onEvent });
  },
  cancel: (requestId: string) => invoke('m5_cancel', { requestId }),
  details: (bindingId: string, messageId: string) => invoke<AiDetails>('m5_details', { bindingId, messageId }),
  directory: (bindingId: string, enabled: boolean) => invoke<DesktopDescriptor[]>('m5_directory', { bindingId, enabled }),
  indexMember: (bindingId: string, grantId: string, count: number, pages: { page: number; text: string }[]) => invoke('m5_index_member', { bindingId, grantId, count, pages }),
  source: (bindingId: string, messageId: string, sourceId: string) => invoke<[DesktopDescriptor, number]>('m5_source', { bindingId, messageId, sourceId }),
};
