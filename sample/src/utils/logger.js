import { LOG_LEVEL } from '../config.js';

const LEVELS = { debug: 0, info: 1, warn: 2, error: 3 };

function shouldLog(level) {
  return LEVELS[level] >= LEVELS[LOG_LEVEL];
}

function formatMessage(level, context, message) {
  const timestamp = new Date().toISOString();
  return `[${timestamp}] [${level.toUpperCase()}] [${context}] ${message}`;
}

export function createLogger(context) {
  return {
    debug(msg, data) {
      if (shouldLog('debug')) console.debug(formatMessage('debug', context, msg), data || '');
    },
    info(msg, data) {
      if (shouldLog('info')) console.info(formatMessage('info', context, msg), data || '');
    },
    warn(msg, data) {
      if (shouldLog('warn')) console.warn(formatMessage('warn', context, msg), data || '');
    },
    error(msg, err) {
      if (shouldLog('error')) console.error(formatMessage('error', context, msg), err || '');
    },
  };
}
