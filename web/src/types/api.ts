export type TtsEngineKind = 'cosyvoice_3' | 'xtts_v2';

export type EngineSelection = 'auto' | TtsEngineKind;

export type AudioRenderFormat = 'ogg' | 'wav';

export type DiscordSnowflake = string | number;

export const PROFILE_SLUG_REGEX = /^[a-z][a-z0-9_-]{1,63}$/;
export const DISCORD_SNOWFLAKE_REGEX = /^\d{1,20}$/;
export const MAX_UINT64_SNOWFLAKE = 18446744073709551615n;

export function isValidProfileSlug(raw: string): boolean {
  return PROFILE_SLUG_REGEX.test(raw.trim());
}

export function isValidDiscordSnowflake(raw: string): boolean {
  const trimmed = raw.trim();
  if (!DISCORD_SNOWFLAKE_REGEX.test(trimmed)) {
    return false;
  }
  try {
    const value = BigInt(trimmed);
    return value > 0n && value <= MAX_UINT64_SNOWFLAKE;
  } catch {
    return false;
  }
}

export interface ProblemDetails {
  type: string;
  title: string;
  status: number;
  detail: string;
}

export interface HealthReadyResponse {
  status: string;
}

export interface RuntimeConfig {
  service_name: string;
  discord_owner_user_id: DiscordSnowflake;
  discord_owner_user_id_str?: string;
  discord_application_id: DiscordSnowflake;
  discord_application_id_str?: string;
  discord_configured?: boolean;
  standalone_desktop_mode?: boolean;
  user_install_url?: string | null;
  bot_dm_url?: string | null;
  default_target_language: string;
  max_cer_bps: number;
  max_tts_attempts: number;
  supported_engines: string[];
  supported_delivery_modes: string[];
}

export interface DetectedGpuInfo {
  index: number;
  name: string;
  vendor: string;
  vram_total_mib: number;
  vram_free_mib: number;
  driver_version: string | null;
}

export interface DesktopHardwarePlan {
  tier: string;
  selected_gpu_index: number | null;
  selected_gpu_name: string | null;
  selected_gpu_vram_mib: number;
  whisper_use_gpu: boolean;
  whisper_model_filename: string;
  llm_model_filename: string;
  llm_gpu_layers: number;
  prewarm_engines: string[];
  estimated_download_mib: number;
  warnings: string[];
}

export interface DesktopSystemStatus {
  standalone_desktop_mode: boolean;
  discord_configured: boolean;
  discord_application_id_str: string;
  discord_owner_user_id_str: string;
  user_install_url: string | null;
  bot_dm_url: string | null;
  studio_env_writable: boolean;
  hardware_specs: {
    os: string;
    arch: string;
    cpu_logical_cores: number;
    system_ram_mib: number;
    free_disk_mib: number;
    gpus: DetectedGpuInfo[];
  };
  hardware_plan: DesktopHardwarePlan;
}

export interface SaveDiscordConfigPayload {
  discord_application_id: string;
  discord_bot_token: string;
  discord_owner_user_id: string;
  verify_online?: boolean;
}

export interface SaveDiscordConfigReceipt {
  saved: boolean;
  verified_bot_username: string | null;
  user_install_url: string;
  bot_dm_url: string;
}

export interface SupportedLanguage {
  code: string;
  english_name: string;
  native_name: string;
  whisper_code: string;
  xtts_code: string;
  supported_engines: TtsEngineKind[];
  recommended_engine: TtsEngineKind;
  phonetic_label: string;
  sample_prompt: string;
}

export interface PagedResponse<T> {
  items: T[];
  total: number;
  limit: number;
  offset: number;
}

export type FinetuneStatus = 'none' | 'queued' | 'running' | 'succeeded' | 'failed';

export type SampleStyleLabel = 'conversational' | 'calm' | 'expressive' | 'crisp';

export interface SynthesisTuning {
  temperature: number;
  top_p: number;
  top_k: number;
  repetition_penalty: number;
  length_penalty: number;
  speed: number;
  gpt_cond_len_secs: number;
  gpt_cond_chunk_len_secs: number;
  max_ref_len_secs: number;
  sound_norm_refs: boolean;
}

export const DEFAULT_SYNTHESIS_TUNING: SynthesisTuning = {
  temperature: 0.62,
  top_p: 0.8,
  top_k: 50,
  repetition_penalty: 5.0,
  length_penalty: 1.0,
  speed: 1.0,
  gpt_cond_len_secs: 30,
  gpt_cond_chunk_len_secs: 4,
  max_ref_len_secs: 60,
  sound_norm_refs: true,
};

export interface SynthesisTuningPreset {
  id: string;
  label: string;
  description: string;
  tuning: SynthesisTuning;
}

export const SYNTHESIS_TUNING_PRESETS: SynthesisTuningPreset[] = [
  {
    id: 'high-likeness',
    label: 'High Likeness (Recommended)',
    description: 'Tighter acoustic sampling for maximum vocal timbre & speaker fidelity.',
    tuning: {
      temperature: 0.58,
      top_p: 0.78,
      top_k: 45,
      repetition_penalty: 5.5,
      length_penalty: 1.0,
      speed: 1.0,
      gpt_cond_len_secs: 30,
      gpt_cond_chunk_len_secs: 4,
      max_ref_len_secs: 60,
      sound_norm_refs: true,
    },
  },
  {
    id: 'balanced-natural',
    label: 'Balanced Natural',
    description: 'Smooth everyday conversational cadence balanced with voice likeness.',
    tuning: {
      temperature: 0.65,
      top_p: 0.82,
      top_k: 50,
      repetition_penalty: 5.0,
      length_penalty: 1.0,
      speed: 1.0,
      gpt_cond_len_secs: 30,
      gpt_cond_chunk_len_secs: 4,
      max_ref_len_secs: 60,
      sound_norm_refs: true,
    },
  },
  {
    id: 'expressive-dynamic',
    label: 'Expressive & Dynamic',
    description: 'Wider pitch & prosody range for animated callouts and emotional lines.',
    tuning: {
      temperature: 0.75,
      top_p: 0.88,
      top_k: 65,
      repetition_penalty: 4.0,
      length_penalty: 1.0,
      speed: 1.0,
      gpt_cond_len_secs: 30,
      gpt_cond_chunk_len_secs: 4,
      max_ref_len_secs: 60,
      sound_norm_refs: true,
    },
  },
];

export interface VoiceProfileSample {
  id: string;
  profile_id: string;
  wav_path: string;
  transcript_en: string;
  style_label: string;
  duration_ms: number;
  is_primary: boolean;
  created_at: string;
}

export interface VoiceProfile {
  id: string;
  slug: string;
  display_name: string;
  avatar_url: string | null;
  reference_wav_path: string;
  reference_transcript_en: string;
  preferred_engine: TtsEngineKind;
  default_target_language: string;
  owner_discord_user_id: DiscordSnowflake;
  owner_discord_user_id_str?: string;
  created_at: string;
  tuning: SynthesisTuning;
  finetune_status: FinetuneStatus;
  finetuned_checkpoint_dir: string | null;
  finetune_error: string | null;
  finetuned_at: string | null;
  samples: VoiceProfileSample[];
  sample_count: number;
  total_sample_duration_ms: number;
}

export interface AdditionalEnrollmentSamplePayload {
  audio_bytes: string;
  transcript_en: string;
  style_label: string;
}

export interface EnrollProfilePayload {
  slug: string;
  display_name: string;
  avatar_url: string | null;
  reference_audio_bytes: string;
  reference_transcript_en: string;
  preferred_engine: TtsEngineKind;
  default_target_language: string;
  additional_samples?: AdditionalEnrollmentSamplePayload[];
  tuning?: SynthesisTuning;
}

export interface UpdateProfilePayload {
  display_name?: string;
  avatar_url?: string | null;
  preferred_engine?: TtsEngineKind;
  default_target_language?: string;
  reference_transcript_en?: string;
  tuning?: SynthesisTuning;
  use_finetuned_checkpoint?: boolean;
}

export interface VoiceFinetuneJob {
  id: string;
  profile_id: string;
  profile_slug?: string;
  status: 'queued' | 'running' | 'succeeded' | 'failed';
  epochs: number;
  learning_rate: number;
  sample_count: number;
  current_epoch: number;
  progress_bps: number;
  last_loss: number | null;
  checkpoint_dir: string | null;
  error_message: string | null;
  created_at: string;
  updated_at: string;
}

export type DiscordDeliveryTarget =
  | { mode: 'client_return_only' }
  | {
      mode: 'profile_webhook';
      webhook_url: string;
      username: string;
      avatar_url: string | null;
    }
  | {
      mode: 'bot_voice_bubble';
      channel_id: string;
      reply_to_message_id: string | null;
    }
  | {
      mode: 'user_install_followup';
      application_id: string;
      interaction_token: string;
      attach_spoiler_pinyin: boolean;
    };

export interface CreateVoiceJobPayload {
  profile_slug: string;
  target_language: string;
  engine_override?: TtsEngineKind | null;
  max_cer_bps_override?: number | null;
  audio_bytes?: string | null;
  source_text?: string | null;
  tuning_override?: SynthesisTuning | null;
  delivery: DiscordDeliveryTarget;
}

export interface VoiceJobReceipt {
  job_id: string;
  profile_id: string;
  profile_slug: string;
  target_language: string;
  delivery_mode: string;
  english_transcript: string;
  spoken_text: string;
  phonetic_guide: string;
  english_back_gloss: string;
  back_transcribed_text: string;
  engine_used: TtsEngineKind;
  cer_bps: number;
  attempts_used: number;
  duration_ms: number;
  total_latency_ms: number;
  output_ogg_path: string;
  discord_message_id: DiscordSnowflake | null;
  discord_message_id_str?: string | null;
  created_at: string;
}

export interface BenchmarkSummary {
  id: string;
  engine_tested: string;
  target_language?: string;
  clips_evaluated: number;
  gate0_accuracy_bps: number;
  digit_hygiene_bps: number;
  mean_cer_bps: number;
  max_clip_cer_bps?: number;
  mean_speaker_sim_bps: number;
  format_compliance_bps: number;
  p95_latency_ms: number;
  peak_vram_mib: number;
  passed_all_gates: boolean;
  scorecard_markdown?: string;
  created_at: string;
}

export interface GuidedReadingScript {
  id: string;
  title: string;
  estimatedSeconds: string;
  transcript: string;
  styleLabel?: SampleStyleLabel;
}

export const ENROLLMENT_READING_SCRIPTS: GuidedReadingScript[] = [
  {
    id: 'rainbow-conversational',
    title: 'Conversational Studio Passage (Balanced Plosives, Sibilants & Vowels)',
    estimatedSeconds: '14–17 sec',
    styleLabel: 'conversational',
    transcript:
      'Hey everyone, let us sync up for fifteen minutes this afternoon before we ship the new voice build. When sunlight strikes raindrops in the air, they act like a prism and form a bright rainbow across the clear blue sky.',
  },
  {
    id: 'warm-casual-call',
    title: 'Casual Discord Call Script (Natural Prosody & Cadence)',
    estimatedSeconds: '13–16 sec',
    styleLabel: 'conversational',
    transcript:
      'Good morning! I am grabbing a quick coffee right now, and then I will jump into the voice channel around seven thirty. Let me know if the audio sounds crisp and warm on your end before we invite the rest of the team.',
  },
  {
    id: 'calm-narrative',
    title: 'Calm Narrative (Steady Low-Variance Resonance & Breath Control)',
    estimatedSeconds: '15–18 sec',
    styleLabel: 'calm',
    transcript:
      'Late in the evening, a gentle breeze drifted across the quiet harbor while distant lanterns reflected softly on the water. Taking a slow, steady breath helps every vowel ring clearly without rushing the end of each phrase.',
  },
  {
    id: 'energetic-callout',
    title: 'Energetic Callout (Upbeat Projection & Dynamic Pitch)',
    estimatedSeconds: '13–16 sec',
    styleLabel: 'expressive',
    transcript:
      'Nice play, team! Rotate over to the north objective right now—we have thirty seconds before the next wave spawns, and our ultimates are completely ready to turn this match around!',
  },
  {
    id: 'crisp-articulation-pace',
    title: 'Crisp Articulation & Pace (Consonant Clusters, Numbers & Technical Clarity)',
    estimatedSeconds: '14–17 sec',
    styleLabel: 'crisp',
    transcript:
      'Version two point four deployed cleanly in forty-eight milliseconds with zero packet loss across twelve regional relays. Please verify each configuration flag carefully before enabling production traffic.',
  },
  {
    id: 'expressive-questions-warmth',
    title: 'Expressive Questions & Warmth (Rising Intonation & Friendly Inflection)',
    estimatedSeconds: '14–17 sec',
    styleLabel: 'expressive',
    transcript:
      'Wait, did you really finish the entire soundcheck in one take? That sounds genuinely amazing! Tell me which microphone position felt most comfortable so we can keep that exact setup for tomorrow.',
  },
];

export const DEFAULT_SUPPORTED_LANGUAGES: SupportedLanguage[] = [
  {
    code: 'es',
    english_name: 'Spanish',
    native_name: 'Español',
    whisper_code: 'es',
    xtts_code: 'es',
    supported_engines: ['xtts_v2'],
    recommended_engine: 'xtts_v2',
    phonetic_label: 'Spanish Stress & Pronunciation Guide',
    sample_prompt: 'Hey team, let us jump into the Discord call at 3:30 PM to review the launch plan!',
  },
  {
    code: 'zh',
    english_name: 'Mandarin Chinese',
    native_name: '中文 (普通话)',
    whisper_code: 'zh',
    xtts_code: 'zh-cn',
    supported_engines: ['cosyvoice_3', 'xtts_v2'],
    recommended_engine: 'cosyvoice_3',
    phonetic_label: 'Pinyin (Tone-Marked)',
    sample_prompt: 'Let us meet at 7:15 tonight and order 3 bowls of beef noodles before the match.',
  },
  {
    code: 'fr',
    english_name: 'French',
    native_name: 'Français',
    whisper_code: 'fr',
    xtts_code: 'fr',
    supported_engines: ['xtts_v2'],
    recommended_engine: 'xtts_v2',
    phonetic_label: 'French Phonetic Guide',
    sample_prompt: 'Good evening! I will share the updated slides in 10 minutes once the build turns green.',
  },
  {
    code: 'de',
    english_name: 'German',
    native_name: 'Deutsch',
    whisper_code: 'de',
    xtts_code: 'de',
    supported_engines: ['xtts_v2'],
    recommended_engine: 'xtts_v2',
    phonetic_label: 'German Pronunciation Guide',
    sample_prompt: 'Everything is running smoothly on our local server with plenty of GPU memory to spare.',
  },
  {
    code: 'it',
    english_name: 'Italian',
    native_name: 'Italiano',
    whisper_code: 'it',
    xtts_code: 'it',
    supported_engines: ['xtts_v2'],
    recommended_engine: 'xtts_v2',
    phonetic_label: 'Italian Stress Guide',
    sample_prompt: 'Sounds great to me! Give me 5 minutes to grab headphones and join the voice room.',
  },
  {
    code: 'pt',
    english_name: 'Portuguese',
    native_name: 'Português',
    whisper_code: 'pt',
    xtts_code: 'pt',
    supported_engines: ['xtts_v2'],
    recommended_engine: 'xtts_v2',
    phonetic_label: 'Portuguese Phonetic Guide',
    sample_prompt: 'Hello friends, can you hear my microphone clearly on the new channel?',
  },
  {
    code: 'ja',
    english_name: 'Japanese',
    native_name: '日本語',
    whisper_code: 'ja',
    xtts_code: 'ja',
    supported_engines: ['cosyvoice_3', 'xtts_v2'],
    recommended_engine: 'cosyvoice_3',
    phonetic_label: 'Romaji',
    sample_prompt: 'Thank you for waiting! I just pushed the fix and will join the lobby in 2 minutes.',
  },
  {
    code: 'ko',
    english_name: 'Korean',
    native_name: '한국어',
    whisper_code: 'ko',
    xtts_code: 'ko',
    supported_engines: ['cosyvoice_3', 'xtts_v2'],
    recommended_engine: 'cosyvoice_3',
    phonetic_label: 'Revised Romanization',
    sample_prompt: 'Are you ready for tonight’s game? Let us warm up for 15 minutes first.',
  },
  {
    code: 'ru',
    english_name: 'Russian',
    native_name: 'Русский',
    whisper_code: 'ru',
    xtts_code: 'ru',
    supported_engines: ['xtts_v2'],
    recommended_engine: 'xtts_v2',
    phonetic_label: 'Cyrillic Transliteration',
    sample_prompt: 'Connection looks solid today, let us test a quick voice clip before starting.',
  },
  {
    code: 'ar',
    english_name: 'Arabic',
    native_name: 'العربية',
    whisper_code: 'ar',
    xtts_code: 'ar',
    supported_engines: ['xtts_v2'],
    recommended_engine: 'xtts_v2',
    phonetic_label: 'Arabic Transliteration',
    sample_prompt: 'Welcome everyone, I am happy to join the conversation this evening.',
  },
  {
    code: 'hi',
    english_name: 'Hindi',
    native_name: 'हिन्दी',
    whisper_code: 'hi',
    xtts_code: 'hi',
    supported_engines: ['xtts_v2'],
    recommended_engine: 'xtts_v2',
    phonetic_label: 'Phonetic Hindi (IAST)',
    sample_prompt: 'Hey there, let us catch up over voice chat right after dinner around 8 PM.',
  },
  {
    code: 'en',
    english_name: 'English',
    native_name: 'English',
    whisper_code: 'en',
    xtts_code: 'en',
    supported_engines: ['cosyvoice_3', 'xtts_v2'],
    recommended_engine: 'xtts_v2',
    phonetic_label: 'Pronunciation & Stress Guide',
    sample_prompt: 'Testing zero-shot English voice cloning with reverse-ASR verification enabled.',
  },
];
