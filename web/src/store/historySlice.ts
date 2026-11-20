import { createAsyncThunk, createSlice, type PayloadAction } from '@reduxjs/toolkit';
import {
  apiListBenchmarks,
  apiListVoiceJobs,
  type ApiConnectionSettings,
} from '../api/client';
import type { BenchmarkSummary, PagedResponse, VoiceJobReceipt } from '../types/api';
import type { ConnectionState } from './connectionSlice';
import { submitVoiceJob } from './studioSlice';

export interface HistoryState {
  jobs: VoiceJobReceipt[];
  jobsTotal: number;
  filterLanguage: string;
  filterProfileSlug: string;
  benchmarks: BenchmarkSummary[];
  benchmarksTotal: number;
  jobsStatus: 'idle' | 'loading' | 'succeeded' | 'failed';
  benchmarksStatus: 'idle' | 'loading' | 'succeeded' | 'failed';
  error: string | null;
}

const initialState: HistoryState = {
  jobs: [],
  jobsTotal: 0,
  filterLanguage: '',
  filterProfileSlug: '',
  benchmarks: [],
  benchmarksTotal: 0,
  jobsStatus: 'idle',
  benchmarksStatus: 'idle',
  error: null,
};

function selectConn(state: { connection: ConnectionState }): ApiConnectionSettings {
  const { apiBaseUrl, bearerToken, discordUserId } = state.connection;
  return { apiBaseUrl, bearerToken, discordUserId };
}

export const fetchVoiceJobs = createAsyncThunk<
  PagedResponse<VoiceJobReceipt>,
  void,
  { state: { connection: ConnectionState; history: HistoryState }; rejectValue: string }
>('history/fetchVoiceJobs', async (_, { getState, rejectWithValue }) => {
  const state = getState();
  try {
    return await apiListVoiceJobs(selectConn(state), {
      limit: 20,
      offset: 0,
      target_language: state.history.filterLanguage || undefined,
      profile_slug: state.history.filterProfileSlug || undefined,
    });
  } catch (err) {
    return rejectWithValue(
      err instanceof Error ? err.message : 'Failed loading voice job history.',
    );
  }
});

export const fetchBenchmarks = createAsyncThunk<
  PagedResponse<BenchmarkSummary>,
  void,
  { state: { connection: ConnectionState }; rejectValue: string }
>('history/fetchBenchmarks', async (_, { getState, rejectWithValue }) => {
  try {
    return await apiListBenchmarks(selectConn(getState()), 10, 0);
  } catch (err) {
    return rejectWithValue(
      err instanceof Error ? err.message : 'Failed loading benchmark runs.',
    );
  }
});

const historySlice = createSlice({
  name: 'history',
  initialState,
  reducers: {
    setFilterLanguage(state, action: PayloadAction<string>) {
      state.filterLanguage = action.payload;
    },
    setFilterProfileSlug(state, action: PayloadAction<string>) {
      state.filterProfileSlug = action.payload;
    },
  },
  extraReducers: (builder) => {
    builder
      .addCase(fetchVoiceJobs.pending, (state) => {
        state.jobsStatus = 'loading';
        state.error = null;
      })
      .addCase(fetchVoiceJobs.fulfilled, (state, action) => {
        state.jobsStatus = 'succeeded';
        state.jobs = action.payload.items;
        state.jobsTotal = action.payload.total;
      })
      .addCase(fetchVoiceJobs.rejected, (state, action) => {
        state.jobsStatus = 'failed';
        state.error = action.payload ?? 'Failed loading job history.';
      })
      .addCase(fetchBenchmarks.pending, (state) => {
        state.benchmarksStatus = 'loading';
      })
      .addCase(fetchBenchmarks.fulfilled, (state, action) => {
        state.benchmarksStatus = 'succeeded';
        state.benchmarks = action.payload.items;
        state.benchmarksTotal = action.payload.total;
      })
      .addCase(fetchBenchmarks.rejected, (state, action) => {
        state.benchmarksStatus = 'failed';
        state.error = action.payload ?? 'Failed loading benchmarks.';
      })
      .addCase(submitVoiceJob.fulfilled, (state, action) => {
        state.jobs = [
          action.payload,
          ...state.jobs.filter((j) => j.job_id !== action.payload.job_id),
        ];
        state.jobsTotal += 1;
      });
  },
});

export const { setFilterLanguage, setFilterProfileSlug } = historySlice.actions;
export const historyReducer = historySlice.reducer;
