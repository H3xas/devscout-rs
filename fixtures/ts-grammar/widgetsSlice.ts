import { createSlice, type PayloadAction } from '@reduxjs/toolkit';

interface WidgetsState {
  items: string[];
  loading: boolean;
}

const initialState: WidgetsState = {
  items: [],
  loading: false,
};

export const widgetsSlice = createSlice({
  name: 'widgets',
  initialState,
  reducers: {
    setLoading(state, action: PayloadAction<boolean>) {
      state.loading = action.payload;
    },
    setItems(state, action: PayloadAction<string[]>) {
      state.items = action.payload;
    },
  },
});

export const { setLoading, setItems } = widgetsSlice.actions;
export default widgetsSlice.reducer;
