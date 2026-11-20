import {
  DEFAULT_SYNTHESIS_TUNING,
  type AudioRenderFormat,
  type BenchmarkSummary,
  type CreateVoiceJobPayload,
  type DesktopSystemStatus,
  type EnrollProfilePayload,
  type FinetuneStatus,
  type HealthReadyResponse,
  type PagedResponse,
  type ProblemDetails,
  type RuntimeConfig,
  type SaveDiscordConfigPayload,
  type SaveDiscordConfigReceipt,
  type SupportedLanguage,
  type SynthesisTuning,
  type TtsEngineKind,
  type UpdateProfilePayload,
  type VoiceFinetuneJob,
  type VoiceJobReceipt,
  type VoiceProfile,
  type VoiceProfileSample,
} from '../types/api';

export interface ApiConnectionSettings {
  apiBaseUrl: string;
  bearerToken: string;
  discordUserId: string;
}

export class ApiProblemError extends Error {
  public readonly status: number;
  public readonly title: string;
  public readonly problemType: string;

  constructor(detail: string, status: number, title = 'API Error', problemType = 'about:blank') {
    super(detail);
    this.name = 'ApiProblemError';
    this.status = status;
    this.title = title;
    this.problemType = problemType;
  }
}

export function normalizeBaseUrl(raw: string): string {
  return raw.trim().replace(/\/+$/, '');
}

export function buildAuthHeaders(
  conn: ApiConnectionSettings,
  extraHeaders?: Record<string, string>,
): Headers {
  const headers = new Headers(extraHeaders);
  if (conn.bearerToken.trim().length > 0) {
    headers.set('Authorization', `Bearer ${conn.bearerToken.trim()}`);
  }
  if (conn.discordUserId.trim().length > 0) {
    headers.set('x-discord-user-id', conn.discordUserId.trim());
  }
  return headers;
}

async function throwForNonOkResponse(response: Response): Promise<never> {
  const rawText = await response.text().catch(() => '');
  if (rawText) {
    try {
      const parsed = JSON.parse(rawText) as Partial<ProblemDetails> & { message?: string };
      if (typeof parsed.detail === 'string' && parsed.detail.trim().length > 0) {
        throw new ApiProblemError(
          parsed.detail,
          parsed.status ?? response.status,
          parsed.title ?? response.statusText,
          parsed.type ?? 'about:blank',
        );
      }
      if (typeof parsed.message === 'string' && parsed.message.trim().length > 0) {
        throw new ApiProblemError(parsed.message, response.status, response.statusText);
      }
    } catch (err) {
      if (err instanceof ApiProblemError) {
        throw err;
      }
    }
    throw new ApiProblemError(rawText, response.status, response.statusText);
  }
  throw new ApiProblemError(
    `HTTP ${response.status} ${response.statusText || 'Request failed'}`.trim(),
    response.status,
    response.statusText,
  );
}

const SNOWFLAKE_JSON_RE =
  /("(?:discord_owner_user_id|discord_application_id|owner_discord_user_id|discord_message_id|channel_id|reply_to_message_id|application_id)"\s*:\s*)(\d{16,20})(?=[,\s}\]])/g;

function parseJsonPreservingSnowflakes<T>(rawJson: string): T {
  const safeJson = rawJson.replace(SNOWFLAKE_JSON_RE, '$1"$2"');
  return JSON.parse(safeJson) as T;
}

function normalizeSnowflake(strValue: unknown, rawValue: unknown): string | null {
  if (typeof strValue === 'string' && strValue.trim().length > 0) {
    return strValue.trim();
  }
  if (typeof rawValue === 'string') {
    const trimmed = rawValue.trim();
    return trimmed.length > 0 ? trimmed : null;
  }
  if (typeof rawValue === 'number' && Number.isFinite(rawValue)) {
    return String(Math.trunc(rawValue));
  }
  return null;
}

function normalizeRuntimeConfig(raw: Record<string, unknown>): RuntimeConfig {
  const ownerId =
    normalizeSnowflake(raw.discord_owner_user_id_str, raw.discord_owner_user_id) ?? '0';
  const appId =
    normalizeSnowflake(raw.discord_application_id_str, raw.discord_application_id) ?? '0';
  return {
    service_name: String(raw.service_name ?? ''),
    discord_owner_user_id: ownerId,
    ...(typeof raw.discord_owner_user_id_str === 'string'
      ? { discord_owner_user_id_str: raw.discord_owner_user_id_str }
      : {}),
    discord_application_id: appId,
    ...(typeof raw.discord_application_id_str === 'string'
      ? { discord_application_id_str: raw.discord_application_id_str }
      : {}),
    ...(typeof raw.discord_configured === 'boolean'
      ? { discord_configured: raw.discord_configured }
      : {}),
    ...(typeof raw.standalone_desktop_mode === 'boolean'
      ? { standalone_desktop_mode: raw.standalone_desktop_mode }
      : {}),
    ...(typeof raw.user_install_url === 'string' || raw.user_install_url === null
      ? { user_install_url: raw.user_install_url as string | null }
      : {}),
    ...(typeof raw.bot_dm_url === 'string' || raw.bot_dm_url === null
      ? { bot_dm_url: raw.bot_dm_url as string | null }
      : {}),
    default_target_language:
      typeof raw.default_target_language === 'string' && raw.default_target_language.length > 0
        ? raw.default_target_language
        : 'zh',
    max_cer_bps: Number(raw.max_cer_bps ?? 1200),
    max_tts_attempts: Number(raw.max_tts_attempts ?? 3),
    supported_engines: Array.isArray(raw.supported_engines)
      ? raw.supported_engines.map(String)
      : ['cosyvoice_3', 'xtts_v2'],
    supported_delivery_modes: Array.isArray(raw.supported_delivery_modes)
      ? raw.supported_delivery_modes.map(String)
      : [],
  };
}

function normalizeNumber(raw: unknown, fallback: number): number {
  const num = Number(raw);
  return raw !== null && raw !== undefined && Number.isFinite(num) ? num : fallback;
}

export function normalizeTuning(raw: unknown): SynthesisTuning {
  if (!raw || typeof raw !== 'object') {
    return { ...DEFAULT_SYNTHESIS_TUNING };
  }
  const obj = raw as Record<string, unknown>;
  return {
    temperature: normalizeNumber(obj.temperature, DEFAULT_SYNTHESIS_TUNING.temperature),
    top_p: normalizeNumber(obj.top_p, DEFAULT_SYNTHESIS_TUNING.top_p),
    top_k: Math.round(normalizeNumber(obj.top_k, DEFAULT_SYNTHESIS_TUNING.top_k)),
    repetition_penalty: normalizeNumber(
      obj.repetition_penalty,
      DEFAULT_SYNTHESIS_TUNING.repetition_penalty,
    ),
    length_penalty: normalizeNumber(
      obj.length_penalty,
      DEFAULT_SYNTHESIS_TUNING.length_penalty,
    ),
    speed: normalizeNumber(obj.speed, DEFAULT_SYNTHESIS_TUNING.speed),
    gpt_cond_len_secs: Math.round(
      normalizeNumber(obj.gpt_cond_len_secs, DEFAULT_SYNTHESIS_TUNING.gpt_cond_len_secs),
    ),
    gpt_cond_chunk_len_secs: Math.round(
      normalizeNumber(
        obj.gpt_cond_chunk_len_secs,
        DEFAULT_SYNTHESIS_TUNING.gpt_cond_chunk_len_secs,
      ),
    ),
    max_ref_len_secs: Math.round(
      normalizeNumber(obj.max_ref_len_secs, DEFAULT_SYNTHESIS_TUNING.max_ref_len_secs),
    ),
    sound_norm_refs:
      typeof obj.sound_norm_refs === 'boolean'
        ? obj.sound_norm_refs
        : DEFAULT_SYNTHESIS_TUNING.sound_norm_refs,
  };
}

function normalizeProfileSample(raw: Record<string, unknown>): VoiceProfileSample {
  return {
    id: String(raw.id ?? ''),
    profile_id: String(raw.profile_id ?? ''),
    wav_path: String(raw.wav_path ?? ''),
    transcript_en: String(raw.transcript_en ?? ''),
    style_label:
      typeof raw.style_label === 'string' && raw.style_label.trim().length > 0
        ? raw.style_label.trim()
        : 'conversational',
    duration_ms: normalizeNumber(raw.duration_ms, 0),
    is_primary: Boolean(raw.is_primary),
    created_at: String(raw.created_at ?? ''),
  };
}

function normalizeFinetuneStatus(raw: unknown): FinetuneStatus {
  if (
    raw === 'queued' ||
    raw === 'running' ||
    raw === 'succeeded' ||
    raw === 'failed'
  ) {
    return raw;
  }
  return 'none';
}

function normalizeFinetuneJob(raw: Record<string, unknown>): VoiceFinetuneJob {
  const statusRaw = raw.status;
  const status: VoiceFinetuneJob['status'] =
    statusRaw === 'running' || statusRaw === 'succeeded' || statusRaw === 'failed'
      ? statusRaw
      : 'queued';
  return {
    id: String(raw.id ?? ''),
    profile_id: String(raw.profile_id ?? ''),
    ...(typeof raw.profile_slug === 'string' ? { profile_slug: raw.profile_slug } : {}),
    status,
    epochs: Math.round(normalizeNumber(raw.epochs, 6)),
    learning_rate: normalizeNumber(raw.learning_rate, 0.000005),
    sample_count: Math.round(normalizeNumber(raw.sample_count, 1)),
    current_epoch: Math.round(normalizeNumber(raw.current_epoch, 0)),
    progress_bps: Math.round(normalizeNumber(raw.progress_bps, 0)),
    last_loss:
      raw.last_loss !== null &&
      raw.last_loss !== undefined &&
      Number.isFinite(Number(raw.last_loss))
        ? Number(raw.last_loss)
        : null,
    checkpoint_dir:
      typeof raw.checkpoint_dir === 'string' ? raw.checkpoint_dir : null,
    error_message:
      typeof raw.error_message === 'string' ? raw.error_message : null,
    created_at: String(raw.created_at ?? ''),
    updated_at: String(raw.updated_at ?? ''),
  };
}

function normalizeProfile(raw: Record<string, unknown>): VoiceProfile {
  const ownerId =
    normalizeSnowflake(raw.owner_discord_user_id_str, raw.owner_discord_user_id) ?? '0';
  const samples = Array.isArray(raw.samples)
    ? raw.samples
        .filter((item): item is Record<string, unknown> => Boolean(item && typeof item === 'object'))
        .map(normalizeProfileSample)
    : [];
  const sampleSumMs = samples.reduce((acc, s) => acc + s.duration_ms, 0);
  const sampleCount =
    typeof raw.sample_count === 'number' && Number.isFinite(raw.sample_count)
      ? Math.max(1, Math.round(raw.sample_count))
      : samples.length || 1;
  const totalSampleDurationMs =
    typeof raw.total_sample_duration_ms === 'number' &&
    Number.isFinite(raw.total_sample_duration_ms)
      ? Math.round(raw.total_sample_duration_ms)
      : sampleSumMs > 0
        ? sampleSumMs
        : 15_000;

  return {
    id: String(raw.id ?? ''),
    slug: String(raw.slug ?? ''),
    display_name: String(raw.display_name ?? ''),
    avatar_url: typeof raw.avatar_url === 'string' ? raw.avatar_url : null,
    reference_wav_path: String(raw.reference_wav_path ?? ''),
    reference_transcript_en: String(raw.reference_transcript_en ?? ''),
    preferred_engine: raw.preferred_engine === 'xtts_v2' ? 'xtts_v2' : 'cosyvoice_3',
    default_target_language:
      typeof raw.default_target_language === 'string' && raw.default_target_language.length > 0
        ? raw.default_target_language
        : 'zh',
    owner_discord_user_id: ownerId,
    ...(typeof raw.owner_discord_user_id_str === 'string'
      ? { owner_discord_user_id_str: raw.owner_discord_user_id_str }
      : {}),
    created_at: String(raw.created_at ?? ''),
    tuning: normalizeTuning(raw.tuning),
    finetune_status: normalizeFinetuneStatus(raw.finetune_status),
    finetuned_checkpoint_dir:
      typeof raw.finetuned_checkpoint_dir === 'string'
        ? raw.finetuned_checkpoint_dir
        : null,
    finetune_error:
      typeof raw.finetune_error === 'string' ? raw.finetune_error : null,
    finetuned_at: typeof raw.finetuned_at === 'string' ? raw.finetuned_at : null,
    samples,
    sample_count: sampleCount,
    total_sample_duration_ms: totalSampleDurationMs,
  };
}

function normalizeReceipt(raw: Record<string, unknown>): VoiceJobReceipt {
  const msgId = normalizeSnowflake(raw.discord_message_id_str, raw.discord_message_id);
  return {
    job_id: String(raw.job_id ?? ''),
    profile_id: String(raw.profile_id ?? ''),
    profile_slug: String(raw.profile_slug ?? ''),
    target_language:
      typeof raw.target_language === 'string' && raw.target_language.length > 0
        ? raw.target_language
        : 'zh',
    delivery_mode: String(raw.delivery_mode ?? 'client_return_only'),
    english_transcript: String(raw.english_transcript ?? ''),
    spoken_text: String(raw.spoken_text ?? raw.spoken_hanzi ?? ''),
    phonetic_guide: String(raw.phonetic_guide ?? raw.pinyin ?? ''),
    english_back_gloss: String(raw.english_back_gloss ?? ''),
    back_transcribed_text: String(raw.back_transcribed_text ?? raw.back_transcribed_zh ?? ''),
    engine_used: raw.engine_used === 'xtts_v2' ? 'xtts_v2' : 'cosyvoice_3',
    cer_bps: Number(raw.cer_bps ?? 0),
    attempts_used: Number(raw.attempts_used ?? 1),
    duration_ms: Number(raw.duration_ms ?? 0),
    total_latency_ms: Number(raw.total_latency_ms ?? 0),
    output_ogg_path: String(raw.output_ogg_path ?? ''),
    discord_message_id: msgId,
    ...(typeof raw.discord_message_id_str === 'string'
      ? { discord_message_id_str: raw.discord_message_id_str }
      : {}),
    created_at: String(raw.created_at ?? ''),
  };
}

function normalizeBenchmark(raw: Record<string, unknown>): BenchmarkSummary {
  return {
    id: String(raw.id ?? raw.run_id ?? ''),
    engine_tested: String(raw.engine_tested ?? ''),
    target_language:
      typeof raw.target_language === 'string' ? raw.target_language : undefined,
    clips_evaluated: Number(raw.clips_evaluated ?? 0),
    gate0_accuracy_bps: Number(raw.gate0_accuracy_bps ?? 0),
    digit_hygiene_bps: Number(raw.digit_hygiene_bps ?? 0),
    mean_cer_bps: Number(raw.mean_cer_bps ?? 0),
    max_clip_cer_bps:
      raw.max_clip_cer_bps !== undefined ? Number(raw.max_clip_cer_bps) : undefined,
    mean_speaker_sim_bps: Number(raw.mean_speaker_sim_bps ?? 0),
    format_compliance_bps: Number(raw.format_compliance_bps ?? 0),
    p95_latency_ms: Number(raw.p95_latency_ms ?? 0),
    peak_vram_mib: Number(raw.peak_vram_mib ?? 0),
    passed_all_gates: Boolean(raw.passed_all_gates),
    scorecard_markdown:
      typeof raw.scorecard_markdown === 'string' ? raw.scorecard_markdown : undefined,
    created_at: String(raw.created_at ?? ''),
  };
}

async function requestJson<T>(
  conn: ApiConnectionSettings,
  path: string,
  init?: RequestInit,
): Promise<T> {
  const base = normalizeBaseUrl(conn.apiBaseUrl);
  const headers = buildAuthHeaders(conn, {
    Accept: 'application/json',
    ...(init?.body ? { 'Content-Type': 'application/json' } : {}),
  });
  const response = await fetch(`${base}${path}`, {
    ...init,
    headers,
  });
  if (!response.ok) {
    await throwForNonOkResponse(response);
  }
  const rawText = await response.text();
  return parseJsonPreservingSnowflakes<T>(rawText);
}

export async function apiGetHealthReady(
  conn: ApiConnectionSettings,
): Promise<HealthReadyResponse> {
  const base = normalizeBaseUrl(conn.apiBaseUrl);
  const response = await fetch(`${base}/health/ready`, {
    method: 'GET',
    headers: { Accept: 'application/json' },
  });
  if (!response.ok) {
    await throwForNonOkResponse(response);
  }
  return (await response.json()) as HealthReadyResponse;
}

export async function apiGetRuntimeConfig(
  conn: ApiConnectionSettings,
): Promise<RuntimeConfig> {
  const raw = await requestJson<Record<string, unknown>>(conn, '/api/v1/config', {
    method: 'GET',
  });
  return normalizeRuntimeConfig(raw);
}

export async function apiGetDesktopSystemStatus(
  conn: ApiConnectionSettings,
): Promise<DesktopSystemStatus | null> {
  try {
    return await requestJson<DesktopSystemStatus>(conn, '/api/v1/system/status', {
      method: 'GET',
    });
  } catch {
    return null;
  }
}

export async function apiSaveDiscordConfig(
  conn: ApiConnectionSettings,
  payload: SaveDiscordConfigPayload,
): Promise<SaveDiscordConfigReceipt> {
  return requestJson<SaveDiscordConfigReceipt>(conn, '/api/v1/system/discord-config', {
    method: 'POST',
    body: JSON.stringify(payload),
  });
}

export async function apiGetLanguages(
  conn: ApiConnectionSettings,
): Promise<SupportedLanguage[]> {
  return requestJson<SupportedLanguage[]>(conn, '/api/v1/languages', { method: 'GET' });
}

export async function apiListProfiles(
  conn: ApiConnectionSettings,
  limit = 50,
  offset = 0,
): Promise<PagedResponse<VoiceProfile>> {
  const raw = await requestJson<PagedResponse<Record<string, unknown>>>(
    conn,
    `/api/v1/profiles?limit=${limit}&offset=${offset}`,
    { method: 'GET' },
  );
  return {
    ...raw,
    items: (raw.items ?? []).map(normalizeProfile),
  };
}

export async function apiEnrollProfile(
  conn: ApiConnectionSettings,
  payload: EnrollProfilePayload,
): Promise<VoiceProfile> {
  const raw = await requestJson<Record<string, unknown>>(conn, '/api/v1/profiles', {
    method: 'POST',
    body: JSON.stringify(payload),
  });
  return normalizeProfile(raw);
}

export async function apiUpdateProfile(
  conn: ApiConnectionSettings,
  slug: string,
  payload: UpdateProfilePayload,
): Promise<VoiceProfile> {
  const raw = await requestJson<Record<string, unknown>>(
    conn,
    `/api/v1/profiles/${encodeURIComponent(slug)}`,
    {
      method: 'PATCH',
      body: JSON.stringify(payload),
    },
  );
  return normalizeProfile(raw);
}

export async function apiDeleteProfile(
  conn: ApiConnectionSettings,
  slug: string,
): Promise<void> {
  const base = normalizeBaseUrl(conn.apiBaseUrl);
  const response = await fetch(`${base}/api/v1/profiles/${encodeURIComponent(slug)}`, {
    method: 'DELETE',
    headers: buildAuthHeaders(conn),
  });
  if (!response.ok) {
    await throwForNonOkResponse(response);
  }
}

export async function apiFetchProfileReferenceAudio(
  conn: ApiConnectionSettings,
  slug: string,
): Promise<Blob> {
  const base = normalizeBaseUrl(conn.apiBaseUrl);
  const response = await fetch(
    `${base}/api/v1/profiles/${encodeURIComponent(slug)}/reference-audio`,
    {
      method: 'GET',
      headers: buildAuthHeaders(conn),
    },
  );
  if (!response.ok) {
    await throwForNonOkResponse(response);
  }
  return response.blob();
}

export async function apiListProfileSamples(
  conn: ApiConnectionSettings,
  slug: string,
): Promise<VoiceProfileSample[]> {
  const raw = await requestJson<unknown>(
    conn,
    `/api/v1/profiles/${encodeURIComponent(slug)}/samples`,
    { method: 'GET' },
  );
  if (Array.isArray(raw)) {
    return raw
      .filter((item): item is Record<string, unknown> => Boolean(item && typeof item === 'object'))
      .map(normalizeProfileSample);
  }
  if (raw && typeof raw === 'object' && Array.isArray((raw as { items?: unknown }).items)) {
    return ((raw as { items: unknown[] }).items)
      .filter(
        (item): item is Record<string, unknown> =>
          Boolean(item && typeof item === 'object' && ('wav_path' in item || 'profile_id' in item)),
      )
      .map(normalizeProfileSample);
  }
  return [];
}

export async function apiAddProfileSample(
  conn: ApiConnectionSettings,
  slug: string,
  payload: {
    audio_bytes: string;
    transcript_en: string;
    style_label?: string;
    make_primary?: boolean;
  },
): Promise<VoiceProfile> {
  const raw = await requestJson<Record<string, unknown>>(
    conn,
    `/api/v1/profiles/${encodeURIComponent(slug)}/samples`,
    {
      method: 'POST',
      body: JSON.stringify(payload),
    },
  );
  return normalizeProfile(raw);
}

export async function apiFetchProfileSampleAudio(
  conn: ApiConnectionSettings,
  slug: string,
  sampleId: string,
): Promise<Blob> {
  const base = normalizeBaseUrl(conn.apiBaseUrl);
  const response = await fetch(
    `${base}/api/v1/profiles/${encodeURIComponent(slug)}/samples/${encodeURIComponent(sampleId)}/audio`,
    {
      method: 'GET',
      headers: buildAuthHeaders(conn),
    },
  );
  if (!response.ok) {
    await throwForNonOkResponse(response);
  }
  return response.blob();
}

export async function apiDeleteProfileSample(
  conn: ApiConnectionSettings,
  slug: string,
  sampleId: string,
): Promise<VoiceProfile> {
  const raw = await requestJson<Record<string, unknown>>(
    conn,
    `/api/v1/profiles/${encodeURIComponent(slug)}/samples/${encodeURIComponent(sampleId)}`,
    { method: 'DELETE' },
  );
  return normalizeProfile(raw);
}

export async function apiStartProfileFinetune(
  conn: ApiConnectionSettings,
  slug: string,
  payload: { epochs?: number; learning_rate?: number },
): Promise<VoiceFinetuneJob> {
  const raw = await requestJson<Record<string, unknown>>(
    conn,
    `/api/v1/profiles/${encodeURIComponent(slug)}/finetune`,
    {
      method: 'POST',
      body: JSON.stringify(payload),
    },
  );
  return normalizeFinetuneJob(raw);
}

export async function apiGetProfileFinetuneStatus(
  conn: ApiConnectionSettings,
  slug: string,
): Promise<VoiceFinetuneJob | null> {
  const base = normalizeBaseUrl(conn.apiBaseUrl);
  const headers = buildAuthHeaders(conn, { Accept: 'application/json' });
  const response = await fetch(
    `${base}/api/v1/profiles/${encodeURIComponent(slug)}/finetune`,
    {
      method: 'GET',
      headers,
    },
  );
  if (response.status === 404 || response.status === 204) {
    return null;
  }
  if (!response.ok) {
    await throwForNonOkResponse(response);
  }
  const rawText = await response.text();
  if (!rawText.trim()) {
    return null;
  }
  const parsed = parseJsonPreservingSnowflakes<unknown>(rawText);
  if (!parsed || typeof parsed !== 'object' || !('epochs' in parsed)) {
    return null;
  }
  return normalizeFinetuneJob(parsed as Record<string, unknown>);
}

export async function apiCreateVoiceJob(
  conn: ApiConnectionSettings,
  payload: CreateVoiceJobPayload,
): Promise<VoiceJobReceipt> {
  const raw = await requestJson<Record<string, unknown>>(conn, '/api/v1/voice-jobs', {
    method: 'POST',
    body: JSON.stringify(payload),
  });
  return normalizeReceipt(raw);
}

export interface ListVoiceJobsQuery {
  limit?: number;
  offset?: number;
  target_language?: string;
  profile_slug?: string;
}

export async function apiListVoiceJobs(
  conn: ApiConnectionSettings,
  query: ListVoiceJobsQuery = {},
): Promise<PagedResponse<VoiceJobReceipt>> {
  const params = new URLSearchParams();
  params.set('limit', String(query.limit ?? 20));
  params.set('offset', String(query.offset ?? 0));
  if (query.target_language && query.target_language.trim().length > 0) {
    params.set('target_language', query.target_language.trim());
  }
  if (query.profile_slug && query.profile_slug.trim().length > 0) {
    params.set('profile_slug', query.profile_slug.trim());
  }
  const raw = await requestJson<PagedResponse<Record<string, unknown>>>(
    conn,
    `/api/v1/voice-jobs?${params.toString()}`,
    { method: 'GET' },
  );
  return {
    ...raw,
    items: (raw.items ?? []).map(normalizeReceipt),
  };
}

export async function apiFetchVoiceJobAudio(
  conn: ApiConnectionSettings,
  jobId: string,
  format: AudioRenderFormat,
): Promise<Blob> {
  const base = normalizeBaseUrl(conn.apiBaseUrl);
  const response = await fetch(
    `${base}/api/v1/voice-jobs/${encodeURIComponent(jobId)}/audio?format=${format}`,
    {
      method: 'GET',
      headers: buildAuthHeaders(conn),
    },
  );
  if (!response.ok) {
    await throwForNonOkResponse(response);
  }
  return response.blob();
}

export interface DirectRenderResult {
  audioBlob: Blob;
  jobId: string | null;
  cerBps: number | null;
  targetLanguage: string | null;
  engineUsed: TtsEngineKind | null;
}

export async function apiRenderVoiceStream(
  conn: ApiConnectionSettings,
  params: {
    profile: string;
    lang: string;
    format: AudioRenderFormat;
    engine?: TtsEngineKind | null;
    audioFile?: Blob | null;
    sourceText?: string | null;
    tuningOverride?: SynthesisTuning | null;
  },
): Promise<DirectRenderResult> {
  const base = normalizeBaseUrl(conn.apiBaseUrl);
  const qs = new URLSearchParams();
  qs.set('profile', params.profile);
  qs.set('lang', params.lang);
  qs.set('format', params.format);
  if (params.engine) {
    qs.set('engine', params.engine);
  }
  if (params.tuningOverride) {
    qs.set('temperature', String(params.tuningOverride.temperature));
    qs.set('top_p', String(params.tuningOverride.top_p));
    qs.set('repetition_penalty', String(params.tuningOverride.repetition_penalty));
    qs.set('speed', String(params.tuningOverride.speed));
    qs.set('gpt_cond_len_secs', String(params.tuningOverride.gpt_cond_len_secs));
    qs.set('sound_norm_refs', String(params.tuningOverride.sound_norm_refs));
  }

  const form = new FormData();
  if (params.audioFile) {
    form.append('audio', params.audioFile, 'clip.wav');
  }
  if (params.sourceText && params.sourceText.trim().length > 0) {
    form.append('source_text', params.sourceText.trim());
  }

  const response = await fetch(`${base}/api/v1/voice-jobs/render?${qs.toString()}`, {
    method: 'POST',
    headers: buildAuthHeaders(conn),
    body: form,
  });
  if (!response.ok) {
    await throwForNonOkResponse(response);
  }

  const cerRaw = response.headers.get('x-cer-bps');
  const engineRaw = response.headers.get('x-engine-used');
  return {
    audioBlob: await response.blob(),
    jobId: response.headers.get('x-job-id'),
    cerBps: cerRaw !== null ? Number(cerRaw) : null,
    targetLanguage: response.headers.get('x-target-language'),
    engineUsed:
      engineRaw === 'xtts_v2' || engineRaw === 'cosyvoice_3' ? engineRaw : null,
  };
}

export async function apiListBenchmarks(
  conn: ApiConnectionSettings,
  limit = 10,
  offset = 0,
): Promise<PagedResponse<BenchmarkSummary>> {
  const raw = await requestJson<PagedResponse<Record<string, unknown>>>(
    conn,
    `/api/v1/benchmarks?limit=${limit}&offset=${offset}`,
    { method: 'GET' },
  );
  return {
    ...raw,
    items: (raw.items ?? []).map(normalizeBenchmark),
  };
}
