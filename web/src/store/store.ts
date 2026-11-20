import { configureStore } from '@reduxjs/toolkit';
import { connectionReducer } from './connectionSlice';
import { languagesReducer } from './languagesSlice';
import { profilesReducer } from './profilesSlice';
import { studioReducer } from './studioSlice';
import { historyReducer } from './historySlice';

export function createAppStore() {
  return configureStore({
    reducer: {
      connection: connectionReducer,
      languages: languagesReducer,
      profiles: profilesReducer,
      studio: studioReducer,
      history: historyReducer,
    },
  });
}

export const store = createAppStore();

export type AppStore = ReturnType<typeof createAppStore>;
export type RootState = ReturnType<AppStore['getState']>;
export type AppDispatch = AppStore['dispatch'];
