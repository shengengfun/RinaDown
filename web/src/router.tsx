// TanStack Router（history 模式）：/login /（任务主界面） /settings。
// 根 beforeLoad 校验 token；未登录一律重定向 /login。

import {
  Outlet,
  createRootRoute,
  createRoute,
  createRouter,
  redirect,
} from '@tanstack/react-router'
import { isAuthenticated } from './lib/auth'
import { LoginScreen } from './routes/login'
import { TasksScreen } from './routes/tasks'
import { SettingsScreen } from './routes/settings'

const rootRoute = createRootRoute({
  component: Outlet,
})

const loginRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/login',
  component: LoginScreen,
  beforeLoad: () => {
    if (isAuthenticated()) throw redirect({ to: '/' })
  },
})

const tasksRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/',
  component: TasksScreen,
  beforeLoad: () => {
    if (!isAuthenticated()) throw redirect({ to: '/login' })
  },
})

const settingsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/settings',
  component: SettingsScreen,
  beforeLoad: () => {
    if (!isAuthenticated()) throw redirect({ to: '/login' })
  },
})

const routeTree = rootRoute.addChildren([loginRoute, tasksRoute, settingsRoute])

export const router = createRouter({ routeTree })

declare module '@tanstack/react-router' {
  interface Register {
    router: typeof router
  }
}
