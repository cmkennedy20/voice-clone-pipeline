import { createAsyncThunk, createSlice } from '@reduxjs/toolkit';
import { apiGetLanguages, type ApiConnectionSettings } from '../api/client';
import { DEFAULT_SUPPORTED_LANGUAGES, type SupportedLanguage } from '../types/api';
import type { ConnectionState } from './connectionSlice';

export interface LanguagesState {
  items: SupportedLanguage[];
  status: 'idle' | 'loading' | 'succeeded' | 'failed';
  error: string | null;
}

const initialState: LanguagesState = {
  items: DEFAULT_SUPPORTED_LANGUAGES,
  status: 'idle',
  error: null,
};

export const fetchLanguages = createAsyncThunk<
  SupportedLanguage[],
  void,
  { state: { connection: ConnectionState }; rejectValue: string }
>('languages/fetchLanguages', async (_, { getState, rejectWithValue }) => {
  const { apiBaseUrl, bearerToken, discordUserId } = getState().connection;
  const conn: ApiConnectionSettings = { apiBaseUrl, bearerToken, discordUserId };
  try {
    const items = await apiGetLanguages(conn);
    return items.length > 0 ? items : DEFAULT_SUPPORTED_LANGUAGES;
  } catch (err) {
    return rejectWithValue(
      err instanceof Error ? err.message : 'Failed loading supported languages.',
    );
  }
});

const languagesSlice = createSlice({
  name: 'languages',
  initialState,
  reducers: {},
  extraReducers: (builder) => {
    builder
      .addCase(fetchLanguages.pending, (state) => {
        state.status = 'loading';
        state.error = null;
      })
      .addCase(fetchLanguages.fulfilled, (state, action) => {
        state.status = 'succeeded';
        state.items = action.payload;
        state.error = null;
      })
      .addCase(fetchLanguages.rejected, (state, action) => {
        state.status = 'failed';
        state.error = action.payload ?? 'Failed loading languages.';
      });
  },
});

export const languagesReducer = languagesSlice.reducer;
