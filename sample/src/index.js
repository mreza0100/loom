import { PORT } from './config.js';
import { initDatabase } from './database.js';
import { registerRoutes } from './routes/api.js';
import { cleanExpiredSessions } from './auth/session.js';
import { createLogger } from './utils/logger.js';

const log = createLogger('app');

function createApp() {
  const routes = { get: {}, post: {}, delete: {} };

  const app = {
    get: (path, handler) => { routes.get[path] = handler; },
    post: (path, handler) => { routes.post[path] = handler; },
    delete: (path, handler) => { routes.delete[path] = handler; },
    handle: async (method, path, req) => {
      const handler = routes[method]?.[path];
      if (!handler) return { status: 404, body: { error: 'Not found' } };
      try {
        const result = await handler(req);
        return { status: 200, body: result };
      } catch (err) {
        return { status: err.statusCode || 500, body: { error: err.message } };
      }
    },
  };

  return app;
}

function startSessionCleanup() {
  setInterval(() => cleanExpiredSessions(), 60_000);
}

export function bootstrap() {
  initDatabase();
  const app = createApp();
  registerRoutes(app);
  startSessionCleanup();
  log.info(`Server ready on port ${PORT}`);
  return app;
}

bootstrap();
