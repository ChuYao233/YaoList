import { createApp } from 'vue'
import './style.css'
import ElementPlus from 'element-plus'
import 'element-plus/dist/index.css'
import App from './App.vue'
import MainPage from './MainPage.vue'
import Login from './components/Login.vue'
import Register from './components/Register.vue'
import FileDetail from './FileDetail.vue'
import AdminPanel from './AdminPanel.vue'
import { createRouter, createWebHistory } from 'vue-router'
import axios from 'axios'

// 配置axios默认设置
axios.defaults.baseURL = ''

// 添加请求拦截器，只对本站API使用credentials
axios.interceptors.request.use(config => {
  // 只对本站的API请求使用credentials
  if (config.url && (config.url.startsWith('/api/') || config.url.startsWith('http://127.0.0.1:3000/api/'))) {
    config.withCredentials = true
  }
  return config
})

const routes = [
  { 
    path: '/', 
    component: MainPage,
    meta: { requiresAuth: false } // 主页不需要强制登录
  },
  { path: '/login', component: Login },
  { path: '/register', component: Register },
  { 
    path: '/admin',
    component: AdminPanel,
    meta: { requiresAuth: true } // 管理后台需要登录
  },
  { 
    path: '/:pathMatch(.*)*', 
    component: MainPage,
    meta: { requiresAuth: false } // 文件浏览不需要强制登录
  }
]

const router = createRouter({
  history: createWebHistory(),
  routes,
})

// 添加导航守卫
router.beforeEach(async (to, from, next) => {
  // 对于不需要认证的页面，直接通过
  if (!to.meta.requiresAuth) {
    next()
    return
  }
  
  // 对于需要认证的页面，检查用户是否已登录
  try {
    const res = await axios.get('/api/user/profile')
    if (res.status === 200 && res.data.username) {
      // 用户已登录
      next()
    } else {
      // 用户未登录，重定向到登录页
      next('/login')
    }
  } catch (error) {
    // 认证失败，重定向到登录页
    next('/login')
  }
})

const app = createApp(App)
app.use(ElementPlus)
app.use(router)
app.mount('#app')
