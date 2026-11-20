import { createAsyncThunk, createSlice, type PayloadAction } from '@reduxjs/toolkit';
import { apiCreateVoiceJob, type ApiConnectionSettings } from '../api/client';
import {
  DEFAULT_SYNTHESIS_TUNING,
  isValidDiscordSnowflake,
  type AudioRenderFormat,
  type CreateVoiceJobPayload,
  type DiscordDeliveryTarget,
  type EngineSelection,
  type SynthesisTuning,
  type VoiceJobReceipt,
} from '../types/api';
import { checkHealthAndConfig, type ConnectionState } from './connectionSlice';
import { deleteProfile, enrollProfile, fetchProfiles } from './profilesSlice';

export type StudioDeliveryMode =
  | 'client_return_only'
  | 'profile_webhook'
  | 'bot_voice_bubble';

export interface StudioState {
  inputMode: 'audio' | 'text';
  sourceText: string;
  selectedProfileSlug: string;
  targetLanguage: string;
  userChoseLanguage: boolean;
  engineSelection: EngineSelection;
  maxCerBpsOverride: string;
  outputFormat: AudioRenderFormat;
  useCustomTuning: boolean;
  customTuning: SynthesisTuning;
  deliveryMode: StudioDeliveryMode;
  webhookUrl: string;
  webhookUsername: string;
  webhookAvatarUrl: string;
  botChannelId: string;
  botReplyMessageId: string;
  lastReceipt: VoiceJobReceipt | null;
  status: 'idle' | 'running' | 'succeeded' | 'failed';
  error: string | null;
}

const initialState: StudioState = {
  inputMode: 'text',
  sourceText: 'Hey team, let us jump into the Discord call at 3:30 PM to review the launch plan!',
  selectedProfileSlug: '',
  targetLanguage: 'zh',
  userChoseLanguage: false,
  engineSelection: 'auto',
  maxCerBpsOverride: '',
  outputFormat: 'ogg',
  useCustomTuning: false,
  customTuning: { ...DEFAULT_SYNTHESIS_TUNING },
  deliveryMode: 'client_return_only',
  webhookUrl: '',
  webhookUsername: 'Voice Clone',
  webhookAvatarUrl: '',
  botChannelId: '',
  botReplyMessageId: '',
  lastReceipt: null,
  status: 'idle',
  error: null,
};

export function buildDeliveryTargetFromStudio(state: StudioState): DiscordDeliveryTarget {
  if (state.deliveryMode === 'profile_webhook') {
    return {
      mode: 'profile_webhook',
      webhook_url: state.webhookUrl.trim(),
      username: state.webhookUsername.trim() || 'Voice Clone',
      avatar_url:
        state.webhookAvatarUrl.trim().length > 0 ? state.webhookAvatarUrl.trim() : null,
    };
  }
  if (state.deliveryMode === 'bot_voice_bubble') {
    const channelRaw = state.botChannelId.trim();
    const replyRaw = state.botReplyMessageId.trim();
    return {
      mode: 'bot_voice_bubble',
      channel_id: channelRaw.length > 0 ? channelRaw : '0',
      reply_to_message_id: replyRaw.length > 0 ? replyRaw : null,
    };
  }
  return { mode: 'client_return_only' };
}

export const submitVoiceJob = createAsyncThunk<
  VoiceJobReceipt,
  { audioBase64?: string | null },
  { state: { connection: ConnectionState; studio: StudioState }; rejectValue: string }
>('studio/submitVoiceJob', async ({ audioBase64 }, { getState, rejectWithValue }) => {
  const { connection, studio } = getState();
  const conn: ApiConnectionSettings = {
    apiBaseUrl: connection.apiBaseUrl,
    bearerToken: connection.bearerToken,
    discordUserId: connection.discordUserId,
  };

  if (!studio.selectedProfileSlug.trim()) {
    return rejectWithValue(
      'Please select or enroll a Voice Profile first before running synthesis.',
    );
  }

  if (studio.inputMode === 'text' && !studio.sourceText.trim()) {
    return rejectWithValue('Please enter English source text to translate and synthesize.');
  }

  if (studio.inputMode === 'audio' && !audioBase64) {
    return rejectWithValue(
      'Please record a voice clip or drop an audio file before running synthesis.',
    );
  }

  if (studio.deliveryMode === 'profile_webhook' && !studio.webhookUrl.trim()) {
    return rejectWithValue('Please provide a Discord Webhook URL for ProfileWebhook delivery.');
  }

  if (studio.deliveryMode === 'bot_voice_bubble') {
    if (!isValidDiscordSnowflake(studio.botChannelId)) {
      return rejectWithValue(
        'Target Discord Channel ID must be a valid 64-bit numeric snowflake (1–20 digits, <= 18446744073709551615).',
      );
    }
    if (
      studio.botReplyMessageId.trim().length > 0 &&
      !isValidDiscordSnowflake(studio.botReplyMessageId)
    ) {
      return rejectWithValue(
        'Reply To Message ID must be a valid 64-bit numeric snowflake (1–20 digits, <= 18446744073709551615).',
      );
    }
  }

  const cerOverrideTrimmed = studio.maxCerBpsOverride.trim();
  const cerOverrideNumber =
    cerOverrideTrimmed.length > 0 ? Number(cerOverrideTrimmed) : null;

  const payload: CreateVoiceJobPayload = {
    profile_slug: studio.selectedProfileSlug.trim(),
    target_language: studio.targetLanguage,
    engine_override: studio.engineSelection === 'auto' ? null : studio.engineSelection,
    max_cer_bps_override:
      cerOverrideNumber !== null && Number.isFinite(cerOverrideNumber)
        ? cerOverrideNumber
        : null,
    audio_bytes: studio.inputMode === 'audio' ? (audioBase64 ?? null) : null,
    source_text: studio.inputMode === 'text' ? studio.sourceText.trim() : null,
    ...(studio.useCustomTuning ? { tuning_override: studio.customTuning } : {}),
    delivery: buildDeliveryTargetFromStudio(studio),
  };

  try {
    return await apiCreateVoiceJob(conn, payload);
  } catch (err) {
    return rejectWithValue(
      err instanceof Error ? err.message : 'Voice synthesis pipeline request failed.',
    );
  }
});

const studioSlice = createSlice({
  name: 'studio',
  initialState,
  reducers: {
    setInputMode(state, action: PayloadAction<'audio' | 'text'>) {
      state.inputMode = action.payload;
      state.error = null;
    },
    setSourceText(state, action: PayloadAction<string>) {
      state.sourceText = action.payload;
    },
    setSelectedProfileSlug(state, action: PayloadAction<string>) {
      state.selectedProfileSlug = action.payload;
    },
    setTargetLanguage(state, action: PayloadAction<string>) {
      state.targetLanguage = action.payload;
      state.userChoseLanguage = true;
    },
    setEngineSelection(state, action: PayloadAction<EngineSelection>) {
      state.engineSelection = action.payload;
    },
    setMaxCerBpsOverride(state, action: PayloadAction<string>) {
      state.maxCerBpsOverride = action.payload;
    },
    setOutputFormat(state, action: PayloadAction<AudioRenderFormat>) {
      state.outputFormat = action.payload;
    },
    setUseCustomTuning(state, action: PayloadAction<boolean>) {
      state.useCustomTuning = action.payload;
    },
    setCustomTuning(state, action: PayloadAction<SynthesisTuning>) {
      state.customTuning = action.payload;
    },
    patchCustomTuning(state, action: PayloadAction<Partial<SynthesisTuning>>) {
      state.customTuning = { ...state.customTuning, ...action.payload };
    },
    setDeliveryMode(state, action: PayloadAction<StudioDeliveryMode>) {
      state.deliveryMode = action.payload;
    },
    setWebhookUrl(state, action: PayloadAction<string>) {
      state.webhookUrl = action.payload;
    },
    setWebhookUsername(state, action: PayloadAction<string>) {
      state.webhookUsername = action.payload;
    },
    setWebhookAvatarUrl(state, action: PayloadAction<string>) {
      state.webhookAvatarUrl = action.payload;
    },
    setBotChannelId(state, action: PayloadAction<string>) {
      state.botChannelId = action.payload;
    },
    setBotReplyMessageId(state, action: PayloadAction<string>) {
      state.botReplyMessageId = action.payload;
    },
    clearStudioError(state) {
      state.error = null;
    },
  },
  extraReducers: (builder) => {
    builder
      .addCase(checkHealthAndConfig.fulfilled, (state, action) => {
        if (!state.userChoseLanguage && action.payload.config.default_target_language) {
          state.targetLanguage = action.payload.config.default_target_language;
        }
      })
      .addCase(fetchProfiles.fulfilled, (state, action) => {
        const first = action.payload.items[0];
        if (!state.selectedProfileSlug && first) {
          state.selectedProfileSlug = first.slug;
        }
      })
      .addCase(enrollProfile.fulfilled, (state, action) => {
        state.selectedProfileSlug = action.payload.slug;
      })
      .addCase(deleteProfile.fulfilled, (state, action) => {
        if (state.selectedProfileSlug === action.payload) {
          state.selectedProfileSlug = '';
        }
      })
      .addCase(submitVoiceJob.pending, (state) => {
        state.status = 'running';
        state.error = null;
      })
      .addCase(submitVoiceJob.fulfilled, (state, action) => {
        state.status = 'succeeded';
        state.lastReceipt = action.payload;
        state.error = null;
      })
      .addCase(submitVoiceJob.rejected, (state, action) => {
        state.status = 'failed';
        state.error = action.payload ?? 'Synthesis job failed.';
      });
  },
});

export const {
  setInputMode,
  setSourceText,
  setSelectedProfileSlug,
  setTargetLanguage,
  setEngineSelection,
  setMaxCerBpsOverride,
  setOutputFormat,
  setUseCustomTuning,
  setCustomTuning,
  patchCustomTuning,
  setDeliveryMode,
  setWebhookUrl,
  setWebhookUsername,
  setWebhookAvatarUrl,
  setBotChannelId,
  setBotReplyMessageId,
  clearStudioError,
} = studioSlice.actions;

export const studioReducer = studioSlice.reducer;
