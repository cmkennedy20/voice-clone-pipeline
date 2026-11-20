import { createAsyncThunk, createSlice, type PayloadAction } from '@reduxjs/toolkit';
import {
  apiAddProfileSample,
  apiDeleteProfile,
  apiDeleteProfileSample,
  apiEnrollProfile,
  apiListProfiles,
  apiStartProfileFinetune,
  apiUpdateProfile,
  type ApiConnectionSettings,
} from '../api/client';
import type {
  EnrollProfilePayload,
  PagedResponse,
  UpdateProfilePayload,
  VoiceFinetuneJob,
  VoiceProfile,
} from '../types/api';
import type { ConnectionState } from './connectionSlice';

export interface ProfilesState {
  items: VoiceProfile[];
  total: number;
  status: 'idle' | 'loading' | 'succeeded' | 'failed';
  mutationStatus: 'idle' | 'saving' | 'succeeded' | 'failed';
  error: string | null;
}

const initialState: ProfilesState = {
  items: [],
  total: 0,
  status: 'idle',
  mutationStatus: 'idle',
  error: null,
};

function selectConn(state: { connection: ConnectionState }): ApiConnectionSettings {
  const { apiBaseUrl, bearerToken, discordUserId } = state.connection;
  return { apiBaseUrl, bearerToken, discordUserId };
}

export const fetchProfiles = createAsyncThunk<
  PagedResponse<VoiceProfile>,
  void,
  { state: { connection: ConnectionState }; rejectValue: string }
>('profiles/fetchProfiles', async (_, { getState, rejectWithValue }) => {
  try {
    return await apiListProfiles(selectConn(getState()), 50, 0);
  } catch (err) {
    return rejectWithValue(
      err instanceof Error ? err.message : 'Failed loading voice profiles.',
    );
  }
});

export const enrollProfile = createAsyncThunk<
  VoiceProfile,
  EnrollProfilePayload,
  { state: { connection: ConnectionState }; rejectValue: string }
>('profiles/enrollProfile', async (payload, { getState, rejectWithValue }) => {
  try {
    return await apiEnrollProfile(selectConn(getState()), payload);
  } catch (err) {
    return rejectWithValue(
      err instanceof Error ? err.message : 'Failed enrolling voice profile.',
    );
  }
});

export const updateProfile = createAsyncThunk<
  VoiceProfile,
  { slug: string; patch: UpdateProfilePayload },
  { state: { connection: ConnectionState }; rejectValue: string }
>('profiles/updateProfile', async ({ slug, patch }, { getState, rejectWithValue }) => {
  try {
    return await apiUpdateProfile(selectConn(getState()), slug, patch);
  } catch (err) {
    return rejectWithValue(
      err instanceof Error ? err.message : 'Failed updating voice profile.',
    );
  }
});

export const deleteProfile = createAsyncThunk<
  string,
  string,
  { state: { connection: ConnectionState }; rejectValue: string }
>('profiles/deleteProfile', async (slug, { getState, rejectWithValue }) => {
  try {
    await apiDeleteProfile(selectConn(getState()), slug);
    return slug;
  } catch (err) {
    return rejectWithValue(
      err instanceof Error ? err.message : 'Failed deleting voice profile.',
    );
  }
});

export const addProfileSample = createAsyncThunk<
  VoiceProfile,
  {
    slug: string;
    payload: {
      audio_bytes: string;
      transcript_en: string;
      style_label?: string;
      make_primary?: boolean;
    };
  },
  { state: { connection: ConnectionState }; rejectValue: string }
>('profiles/addProfileSample', async ({ slug, payload }, { getState, rejectWithValue }) => {
  try {
    return await apiAddProfileSample(selectConn(getState()), slug, payload);
  } catch (err) {
    return rejectWithValue(
      err instanceof Error ? err.message : 'Failed adding reference sample to profile.',
    );
  }
});

export const deleteProfileSample = createAsyncThunk<
  VoiceProfile,
  { slug: string; sampleId: string },
  { state: { connection: ConnectionState }; rejectValue: string }
>(
  'profiles/deleteProfileSample',
  async ({ slug, sampleId }, { getState, rejectWithValue }) => {
    try {
      return await apiDeleteProfileSample(selectConn(getState()), slug, sampleId);
    } catch (err) {
      return rejectWithValue(
        err instanceof Error ? err.message : 'Failed removing reference sample.',
      );
    }
  },
);

export const startProfileFinetune = createAsyncThunk<
  { slug: string; job: VoiceFinetuneJob },
  { slug: string; epochs?: number; learning_rate?: number },
  { state: { connection: ConnectionState }; rejectValue: string }
>(
  'profiles/startProfileFinetune',
  async ({ slug, epochs, learning_rate }, { getState, rejectWithValue }) => {
    try {
      const job = await apiStartProfileFinetune(selectConn(getState()), slug, {
        epochs,
        learning_rate,
      });
      return { slug, job };
    } catch (err) {
      return rejectWithValue(
        err instanceof Error ? err.message : 'Failed starting RTX 3090 fine-tuning job.',
      );
    }
  },
);

const profilesSlice = createSlice({
  name: 'profiles',
  initialState,
  reducers: {
    clearProfilesError(state) {
      state.error = null;
    },
    upsertProfile(state, action: PayloadAction<VoiceProfile>) {
      state.items = state.items.map((p) =>
        p.slug === action.payload.slug ? action.payload : p,
      );
    },
  },
  extraReducers: (builder) => {
    builder
      .addCase(fetchProfiles.pending, (state) => {
        state.status = 'loading';
        state.error = null;
      })
      .addCase(fetchProfiles.fulfilled, (state, action) => {
        state.status = 'succeeded';
        state.items = action.payload.items;
        state.total = action.payload.total;
      })
      .addCase(fetchProfiles.rejected, (state, action) => {
        state.status = 'failed';
        state.error = action.payload ?? 'Failed loading profiles.';
      })
      .addCase(enrollProfile.pending, (state) => {
        state.mutationStatus = 'saving';
        state.error = null;
      })
      .addCase(enrollProfile.fulfilled, (state, action) => {
        state.mutationStatus = 'succeeded';
        state.items = [
          action.payload,
          ...state.items.filter((p) => p.slug !== action.payload.slug),
        ];
        state.total += 1;
      })
      .addCase(enrollProfile.rejected, (state, action) => {
        state.mutationStatus = 'failed';
        state.error = action.payload ?? 'Failed enrolling profile.';
      })
      .addCase(updateProfile.pending, (state) => {
        state.mutationStatus = 'saving';
        state.error = null;
      })
      .addCase(updateProfile.fulfilled, (state, action) => {
        state.mutationStatus = 'succeeded';
        state.items = state.items.map((p) =>
          p.slug === action.payload.slug ? action.payload : p,
        );
      })
      .addCase(updateProfile.rejected, (state, action) => {
        state.mutationStatus = 'failed';
        state.error = action.payload ?? 'Failed updating profile.';
      })
      .addCase(deleteProfile.pending, (state) => {
        state.mutationStatus = 'saving';
        state.error = null;
      })
      .addCase(deleteProfile.fulfilled, (state, action) => {
        state.mutationStatus = 'succeeded';
        state.items = state.items.filter((p) => p.slug !== action.payload);
        state.total = Math.max(0, state.total - 1);
      })
      .addCase(deleteProfile.rejected, (state, action) => {
        state.mutationStatus = 'failed';
        state.error = action.payload ?? 'Failed deleting profile.';
      })
      .addCase(addProfileSample.pending, (state) => {
        state.mutationStatus = 'saving';
        state.error = null;
      })
      .addCase(addProfileSample.fulfilled, (state, action) => {
        state.mutationStatus = 'succeeded';
        state.items = state.items.map((p) =>
          p.slug === action.payload.slug ? action.payload : p,
        );
      })
      .addCase(addProfileSample.rejected, (state, action) => {
        state.mutationStatus = 'failed';
        state.error = action.payload ?? 'Failed adding sample.';
      })
      .addCase(deleteProfileSample.pending, (state) => {
        state.mutationStatus = 'saving';
        state.error = null;
      })
      .addCase(deleteProfileSample.fulfilled, (state, action) => {
        state.mutationStatus = 'succeeded';
        state.items = state.items.map((p) =>
          p.slug === action.payload.slug ? action.payload : p,
        );
      })
      .addCase(deleteProfileSample.rejected, (state, action) => {
        state.mutationStatus = 'failed';
        state.error = action.payload ?? 'Failed deleting sample.';
      })
      .addCase(startProfileFinetune.pending, (state) => {
        state.mutationStatus = 'saving';
        state.error = null;
      })
      .addCase(startProfileFinetune.fulfilled, (state, action) => {
        state.mutationStatus = 'succeeded';
        state.items = state.items.map((p) =>
          p.slug === action.payload.slug
            ? {
                ...p,
                finetune_status: action.payload.job.status,
                finetuned_checkpoint_dir:
                  action.payload.job.checkpoint_dir ?? p.finetuned_checkpoint_dir,
              }
            : p,
        );
      })
      .addCase(startProfileFinetune.rejected, (state, action) => {
        state.mutationStatus = 'failed';
        state.error = action.payload ?? 'Failed starting fine-tuning.';
      });
  },
});

export const { clearProfilesError, upsertProfile } = profilesSlice.actions;
export const profilesReducer = profilesSlice.reducer;
