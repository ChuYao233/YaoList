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
router.beforeEach((to, from, next) => {
  const isAuthenticated = !!localStorage.getItem('yaolist_user')
  
  if (to.meta.requiresAuth && !isAuthenticated) {
    // 如果需要认证但用户未登录，重定向到登录页
    next('/login')
  } else if (to.path === '/login' && isAuthenticated) {
    // 如果用户已登录但访问登录页，重定向到主页
    next('/')
  } else {
    next()
  }
})

const app = createApp(App)
app.use(ElementPlus)
app.use(router)
app.mount('#app')
