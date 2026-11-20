import type {
  AudioRenderFormat,
  DiscordDeliveryTarget,
  EngineSelection,
  SynthesisTuning,
} from '../types/api';
import { normalizeBaseUrl } from './client';

export const DEFAULT_LOCAL_API_ORIGIN = 'http://localhost:8080';

export interface CurlSnippetInput {
  apiBaseUrl: string;
  discordUserId: string;
  profileSlug: string;
  targetLanguage: string;
  engineSelection: EngineSelection;
  maxCerBpsOverride: string;
  inputMode: 'audio' | 'text';
  sourceText: string;
  outputFormat: AudioRenderFormat;
  delivery: DiscordDeliveryTarget;
  tuningOverride?: SynthesisTuning | null;
}

function resolveCurlOrigin(apiBaseUrl: string): string {
  const explicit = normalizeBaseUrl(apiBaseUrl);
  if (explicit) {
    return explicit;
  }
  if (
    typeof window === 'undefined' ||
    !window.location?.origin ||
    window.location.origin === 'null'
  ) {
    return DEFAULT_LOCAL_API_ORIGIN;
  }
  const { hostname, origin } = window.location;
  if (hostname === 'localhost' || hostname === '127.0.0.1' || hostname === '[::1]') {
    return DEFAULT_LOCAL_API_ORIGIN;
  }
  return origin;
}

function resolveCurlFlags(origin: string): string {
  try {
    const parsed = new URL(origin);
    if (parsed.protocol === 'https:') {
      const isIpLiteral =
        /^\d{1,3}(?:\.\d{1,3}){3}$/.test(parsed.hostname) ||
        parsed.hostname.startsWith('[');
      const isNonStandardPort = parsed.port !== '' && parsed.port !== '443';
      if (isIpLiteral || isNonStandardPort || parsed.hostname === 'localhost') {
        return '-ksS';
      }
    }
  } catch {
    if (origin.startsWith('https://') && origin.includes(':3443')) {
      return '-ksS';
    }
  }
  return '-sS';
}

export function buildVoiceJobJsonCurl(input: CurlSnippetInput): string {
  const origin = resolveCurlOrigin(input.apiBaseUrl);
  const flags = resolveCurlFlags(origin);

  const body: Record<string, unknown> = {
    profile_slug: input.profileSlug || 'default_voice',
    target_language: input.targetLanguage,
    engine_override: input.engineSelection === 'auto' ? null : input.engineSelection,
    max_cer_bps_override:
      input.maxCerBpsOverride.trim().length > 0 ? Number(input.maxCerBpsOverride) : null,
    ...(input.inputMode === 'text'
      ? {
          source_text:
            input.sourceText.trim() ||
            'Hello from the Polyglot Voice Clone Studio!',
          audio_bytes: null,
        }
      : {
          audio_bytes: '<BASE64_WAV_OR_AUDIO_BYTES>',
          source_text: null,
        }),
    ...(input.tuningOverride ? { tuning_override: input.tuningOverride } : {}),
    delivery: input.delivery,
  };

  const discordHeaderLine =
    input.discordUserId.trim().length > 0
      ? `  -H "x-discord-user-id: ${input.discordUserId.trim()}" \\\n`
      : '';

  return [
    `curl ${flags} -X POST "${origin}/api/v1/voice-jobs" \\`,
    `  -H "Authorization: Bearer $OWNER_API_BEARER_TOKEN" \\`,
    `${discordHeaderLine}  -H "Content-Type: application/json" \\`,
    `  -d '${JSON.stringify(body, null, 2)}'`,
  ].join('\n');
}

export function buildDirectRenderMultipartCurl(input: CurlSnippetInput): string {
  const origin = resolveCurlOrigin(input.apiBaseUrl);
  const flags = resolveCurlFlags(origin);

  const qs = new URLSearchParams();
  qs.set('profile', input.profileSlug || 'default_voice');
  qs.set('lang', input.targetLanguage);
  qs.set('format', input.outputFormat);
  if (input.engineSelection !== 'auto') {
    qs.set('engine', input.engineSelection);
  }
  if (input.tuningOverride) {
    qs.set('temperature', String(input.tuningOverride.temperature));
    qs.set('top_p', String(input.tuningOverride.top_p));
    qs.set('repetition_penalty', String(input.tuningOverride.repetition_penalty));
    qs.set('speed', String(input.tuningOverride.speed));
    qs.set('gpt_cond_len_secs', String(input.tuningOverride.gpt_cond_len_secs));
    qs.set('sound_norm_refs', String(input.tuningOverride.sound_norm_refs));
  }

  const formLine =
    input.inputMode === 'text'
      ? `  -F "source_text=${(input.sourceText.trim() || 'Hello team!').replace(/"/g, '\\"')}" \\`
      : `  -F "audio=@my_english_clip.wav" \\`;

  const outExt = input.outputFormat === 'wav' ? 'cloned_48k.wav' : 'cloned_voice.ogg';
  const discordHeaderLine =
    input.discordUserId.trim().length > 0
      ? `  -H "x-discord-user-id: ${input.discordUserId.trim()}" \\\n`
      : '';

  return [
    `curl ${flags} -X POST "${origin}/api/v1/voice-jobs/render?${qs.toString()}" \\`,
    `  -H "Authorization: Bearer $OWNER_API_BEARER_TOKEN" \\`,
    `${discordHeaderLine}${formLine}`,
    `  --output ${outExt}`,
  ].join('\n');
}
