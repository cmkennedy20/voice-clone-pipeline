import { createAsyncThunk, createSlice, type PayloadAction } from '@reduxjs/toolkit';
import {
  apiGetHealthReady,
  apiGetRuntimeConfig,
  type ApiConnectionSettings,
} from '../api/client';
import type { RuntimeConfig } from '../types/api';

export interface ConnectionState extends ApiConnectionSettings {
  healthStatus: 'unknown' | 'checking' | 'ready' | 'offline';
  runtimeConfig: RuntimeConfig | null;
  error: string | null;
}

const TOKEN_STORAGE_KEY = 'voice_clone_studio_bearer_token';
export const DEFAULT_PROXY_BEARER_SENTINEL = 'dev-owner-bearer-token-change-me';

function resolveInitialBearerToken(): string {
  if (typeof window !== 'undefined') {
    try {
      const url = new URL(window.location.href);
      const queryToken = url.searchParams.get('token')?.trim();
      if (queryToken) {
        window.localStorage?.setItem(TOKEN_STORAGE_KEY, queryToken);
        url.searchParams.delete('token');
        window.history?.replaceState({}, '', `${url.pathname}${url.search}${url.hash}`);
        return queryToken;
      }
      const saved = window.localStorage?.getItem(TOKEN_STORAGE_KEY)?.trim();
      if (saved) {
        return saved;
      }
    } catch {
      // Ignore URL / storage errors in restricted environments.
    }
  }
  return DEFAULT_PROXY_BEARER_SENTINEL;
}

const initialState: ConnectionState = {
  apiBaseUrl: '',
  bearerToken: resolveInitialBearerToken(),
  discordUserId: '',
  healthStatus: 'unknown',
  runtimeConfig: null,
  error: null,
};

export const checkHealthAndConfig = createAsyncThunk<
  { ready: boolean; config: RuntimeConfig },
  void,
  { state: { connection: ConnectionState }; rejectValue: string }
>('connection/checkHealthAndConfig', async (_, { getState, rejectWithValue }) => {
  const { apiBaseUrl, bearerToken, discordUserId } = getState().connection;
  const conn: ApiConnectionSettings = { apiBaseUrl, bearerToken, discordUserId };
  try {
    const [health, config] = await Promise.all([
      apiGetHealthReady(conn),
      apiGetRuntimeConfig(conn),
    ]);
    return {
      ready: health.status === 'ready',
      config,
    };
  } catch (err) {
    return rejectWithValue(
      err instanceof Error ? err.message : 'Unable to reach API server.',
    );
  }
});

const connectionSlice = createSlice({
  name: 'connection',
  initialState,
  reducers: {
    setApiBaseUrl(state, action: PayloadAction<string>) {
      state.apiBaseUrl = action.payload;
    },
    setBearerToken(state, action: PayloadAction<string>) {
      state.bearerToken = action.payload;
      if (typeof window !== 'undefined') {
        try {
          const trimmed = action.payload.trim();
          if (trimmed && trimmed !== DEFAULT_PROXY_BEARER_SENTINEL) {
            window.localStorage?.setItem(TOKEN_STORAGE_KEY, trimmed);
          } else {
            window.localStorage?.removeItem(TOKEN_STORAGE_KEY);
          }
        } catch {
          // Ignore storage quota / security errors.
        }
      }
    },
    setDiscordUserId(state, action: PayloadAction<string>) {
      state.discordUserId = action.payload;
    },
    clearConnectionError(state) {
      state.error = null;
    },
  },
  extraReducers: (builder) => {
    builder
      .addCase(checkHealthAndConfig.pending, (state) => {
        state.healthStatus = 'checking';
        state.error = null;
      })
      .addCase(checkHealthAndConfig.fulfilled, (state, action) => {
        state.healthStatus = action.payload.ready ? 'ready' : 'offline';
        state.runtimeConfig = action.payload.config;
        state.error = null;
      })
      .addCase(checkHealthAndConfig.rejected, (state, action) => {
        state.healthStatus = 'offline';
        state.error = action.payload ?? 'Connection check failed.';
      });
  },
});

export const {
  setApiBaseUrl,
  setBearerToken,
  setDiscordUserId,
  clearConnectionError,
} = connectionSlice.actions;

export const connectionReducer = connectionSlice.reducer;
