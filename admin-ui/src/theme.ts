import type { ThemeConfig } from 'antd';
import { theme as antTheme } from 'antd';

const shared = {
  colorPrimary: '#1677ff',
  borderRadius: 6,
};

export const lightTheme: ThemeConfig = {
  token: shared,
  algorithm: antTheme.defaultAlgorithm,
};

export const darkTheme: ThemeConfig = {
  token: shared,
  algorithm: antTheme.darkAlgorithm,
};
