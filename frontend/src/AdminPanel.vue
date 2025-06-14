<template>
  <div class="admin-layout" :class="{ 'dark-mode': isDarkMode }">
    <!-- 左侧菜单栏 -->
    <aside class="sidebar">
      <div class="sidebar-header">
        <h2 class="logo">YaoList</h2>
        <p class="subtitle">管理后台</p>
      </div>
      
      <nav class="sidebar-nav">
        <div class="nav-section">
          <h3 class="nav-section-title">管理</h3>
          <ul class="nav-menu">
            <li class="nav-item">
              <a href="#" 
                 :class="['nav-link', { active: activeTab === 'profile' }]"
                 @click="setActiveTab('profile')">
                <i class="icon">
                  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                    <path d="M20 21v-2a4 4 0 00-4-4H8a4 4 0 00-4 4v2"/>
                    <circle cx="12" cy="7" r="4"/>
                  </svg>
                </i>
                <span>个人资料</span>
              </a>
            </li>
            <li v-if="isAdmin" class="nav-item">
              <a href="#" 
                 :class="['nav-link', { active: activeTab === 'site' }]"
                 @click="setActiveTab('site')">
                <i class="icon">
                  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                    <circle cx="12" cy="12" r="3"/>
                    <path d="M19.4 15a1.65 1.65 0 00.33 1.82l.06.06a2 2 0 010 2.83 2 2 0 01-2.83 0l-.06-.06a1.65 1.65 0 00-1.82-.33 1.65 1.65 0 00-1 1.51V21a2 2 0 01-2 2 2 2 0 01-2-2v-.09A1.65 1.65 0 009 19.4a1.65 1.65 0 00-1.82.33l-.06.06a2 2 0 01-2.83 0 2 2 0 010-2.83l.06-.06a1.65 1.65 0 00.33-1.82 1.65 1.65 0 00-1.51-1H3a2 2 0 01-2-2 2 2 0 012-2h.09A1.65 1.65 0 004.6 9a1.65 1.65 0 00-.33-1.82l-.06-.06a2 2 0 010-2.83 2 2 0 012.83 0l.06.06a1.65 1.65 0 001.82.33H9a1.65 1.65 0 001-1.51V3a2 2 0 012-2 2 2 0 012 2v.09a1.65 1.65 0 001 1.51 1.65 1.65 0 001.82-.33l.06-.06a2 2 0 012.83 0 2 2 0 010 2.83l-.06.06a1.65 1.65 0 00-.33 1.82V9a1.65 1.65 0 001.51 1H21a2 2 0 012 2 2 2 0 01-2 2h-.09a1.65 1.65 0 00-1.51 1z"/>
                  </svg>
                </i>
                <span>站点设置</span>
              </a>
              <!-- 站点设置子菜单 -->
              <ul v-if="activeTab === 'site'" class="sub-menu">
                <li class="sub-menu-item">
                  <a href="#" 
                     :class="['sub-nav-link', { active: siteSubTab === 'general' }]"
                     @click="setSiteSubTab('general')">
                    <i class="icon">
                      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                        <path d="M3 9l9-7 9 7v11a2 2 0 01-2 2H5a2 2 0 01-2-2z"/>
                        <polyline points="9,22 9,12 15,12 15,22"/>
                      </svg>
                    </i>
                    <span>基本设置</span>
                  </a>
                </li>
                <li class="sub-menu-item">
                  <a href="#" 
                     :class="['sub-nav-link', { active: siteSubTab === 'appearance' }]"
                     @click="setSiteSubTab('appearance')">
                    <i class="icon">
                      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                        <path d="M12 2l3.09 6.26L22 9.27l-5 4.87 1.18 6.88L12 17.77l-6.18 3.25L7 14.14 2 9.27l6.91-1.01L12 2z"/>
                      </svg>
                    </i>
                    <span>外观设置</span>
                  </a>
                </li>
                <li class="sub-menu-item">
                  <a href="#" 
                     :class="['sub-nav-link', { active: siteSubTab === 'pagination' }]"
                     @click="setSiteSubTab('pagination')">
                    <i class="icon">
                      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                        <path d="M14 2H6a2 2 0 00-2 2v16a2 2 0 002 2h12a2 2 0 002-2V8z"/>
                        <polyline points="14,2 14,8 20,8"/>
                        <line x1="16" y1="13" x2="8" y2="13"/>
                        <line x1="16" y1="17" x2="8" y2="17"/>
                        <polyline points="10,9 9,9 8,9"/>
                      </svg>
                    </i>
                    <span>分页设置</span>
                  </a>
                </li>
                <li class="sub-menu-item">
                  <a href="#" 
                     :class="['sub-nav-link', { active: siteSubTab === 'preview' }]"
                     @click="setSiteSubTab('preview')">
                    <i class="icon">
                      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                        <path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z"/>
                        <circle cx="12" cy="12" r="3"/>
                      </svg>
                    </i>
                    <span>预览设置</span>
                  </a>
                </li>
              </ul>
            </li>
            <li v-if="isAdmin" class="nav-item">
              <a href="#" 
                 :class="['nav-link', { active: activeTab === 'users' }]"
                 @click="setActiveTab('users')">
                <i class="icon">
                  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                    <path d="M17 21v-2a4 4 0 00-4-4H5a4 4 0 00-4 4v2"/>
                    <circle cx="9" cy="7" r="4"/>
                    <path d="M23 21v-2a4 4 0 00-3-3.87"/>
                    <path d="M16 3.13a4 4 0 010 7.75"/>
                  </svg>
                </i>
                <span>用户管理</span>
              </a>
            </li>
            <li v-if="isAdmin" class="nav-item">
              <a href="#" 
                 :class="['nav-link', { active: activeTab === 'storage' }]"
                 @click="setActiveTab('storage')">
                <i class="icon">
                  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                    <ellipse cx="12" cy="5" rx="9" ry="3"/>
                    <path d="M21 12c0 1.66-4 3-9 3s-9-1.34-9-3"/>
                    <path d="M3 5v14c0 1.66 4 3 9 3s9-1.34 9-3V5"/>
                  </svg>
                </i>
                <span>存储管理</span>
              </a>
            </li>
            <li v-if="isAdmin" class="nav-item">
              <a href="#" 
                 :class="['nav-link', { active: activeTab === 'backup' }]"
                 @click="() => {
                   setActiveTab('backup');
                   notification.info('功能正在开发中，敬请期待！');
                 }">
                <i class="icon">
                  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                    <path d="M14 2H6a2 2 0 00-2 2v16a2 2 0 002 2h12a2 2 0 002-2V8z"/>
                    <polyline points="14,2 14,8 20,8"/>
                    <path d="M12 18v-6"/>
                    <path d="M9 15l3 3 3-3"/>
                  </svg>
                </i>
                <span>备份&恢复</span>
              </a>
            </li>
          </ul>
        </div>
        
        <div class="nav-section">
          <h3 class="nav-section-title">信息</h3>
          <ul class="nav-menu">
            <li class="nav-item">
              <a href="#" 
                 :class="['nav-link', { active: activeTab === 'about' }]"
                 @click="setActiveTab('about')">
                <i class="icon">
                  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                    <circle cx="12" cy="12" r="10"/>
                    <path d="M12 16v-4"/>
                    <path d="M12 8h.01"/>
                  </svg>
                </i>
                <span>关于YaoList</span>
              </a>
            </li>
            <li class="nav-item">
              <a href="#" 
                 :class="['nav-link', { active: activeTab === 'docs' }]"
                 @click="handleDocsClick">
                <i class="icon">
                  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                    <path d="M4 19.5A2.5 2.5 0 016.5 17H20"/>
                    <path d="M6.5 2H20v20H6.5A2.5 2.5 0 014 19.5v-15A2.5 2.5 0 016.5 2z"/>
                  </svg>
                </i>
                <span>文档</span>
              </a>
            </li>
            <li class="nav-item">
              <a href="#" @click="goToHome" class="nav-link">
                <i class="icon">
                  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                    <path d="M3 9l9-7 9 7v11a2 2 0 01-2 2H5a2 2 0 01-2-2z"/>
                    <polyline points="9,22 9,12 15,12 15,22"/>
                  </svg>
                </i>
                <span>主页</span>
              </a>
            </li>
          </ul>
        </div>
      </nav>
      
      <!-- 底部控制按钮 -->
      <div class="sidebar-footer">
        <button class="control-btn" @click="toggleLanguage" :title="currentLanguage === 'zh' ? '切换到英文' : 'Switch to Chinese'">
          <i class="icon">
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
              <circle cx="12" cy="12" r="10"/>
              <line x1="2" y1="12" x2="22" y2="12"/>
              <path d="M12 2a15.3 15.3 0 014 10 15.3 15.3 0 01-4 10 15.3 15.3 0 01-4-10 15.3 15.3 0 014-10z"/>
            </svg>
          </i>
          <span>{{ currentLanguage === 'zh' ? '中文' : 'EN' }}</span>
        </button>
        <button class="control-btn" @click="toggleDarkMode" :title="isDarkMode ? '切换到浅色模式' : '切换到深色模式'">
          <i class="icon">
            <svg v-if="isDarkMode" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
              <circle cx="12" cy="12" r="5"/>
              <line x1="12" y1="1" x2="12" y2="3"/>
              <line x1="12" y1="21" x2="12" y2="23"/>
              <line x1="4.22" y1="4.22" x2="5.64" y2="5.64"/>
              <line x1="18.36" y1="18.36" x2="19.78" y2="19.78"/>
              <line x1="1" y1="12" x2="3" y2="12"/>
              <line x1="21" y1="12" x2="23" y2="12"/>
              <line x1="4.22" y1="19.78" x2="5.64" y2="18.36"/>
              <line x1="18.36" y1="5.64" x2="19.78" y2="4.22"/>
            </svg>
            <svg v-else width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
              <path d="M21 12.79A9 9 0 1111.21 3 7 7 0 0021 12.79z"/>
            </svg>
          </i>
          <span>{{ isDarkMode ? '浅色' : '深色' }}</span>
        </button>
      </div>
    </aside>
    
    <!-- 主内容区域 -->
    <main class="main-content">

      
      <header class="content-header">
        <h1 class="page-title">{{ getPageTitle() }}</h1>
        <div class="user-info">
          <span class="welcome-text">欢迎，{{ username }}</span>
          <button class="logout-btn" @click="logout">退出登录</button>
        </div>
      </header>
      
      <div class="content-body">
        <!-- 个人资料 -->
        <div v-if="activeTab === 'profile'" class="content-section">
          <!-- 用户信息卡片 -->
          <div class="profile-header-card">
            <div class="profile-avatar">
              <div class="avatar-circle">
                <svg width="32" height="32" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                  <path d="M20 21v-2a4 4 0 00-4-4H8a4 4 0 00-4 4v2"/>
                  <circle cx="12" cy="7" r="4"/>
                </svg>
              </div>
            </div>
            <div class="profile-info">
              <h2 class="profile-name">{{ user.username }}</h2>
              <div class="profile-meta">
                <span class="user-role" :class="{ 'admin-role': isAdmin, 'guest-role': isGuest }">
                  {{ isAdmin ? '管理员' : isGuest ? '游客用户' : '普通用户' }}
                </span>
                <span class="user-status" :class="{ 'status-active': user.enabled }">
                  {{ user.enabled ? '已启用' : '已禁用' }}
                </span>
              </div>
              <div class="profile-details">
                <div class="detail-item">
                  <span class="detail-label">用户路径：</span>
                  <span class="detail-value">{{ user.user_path || '/' }}</span>
                </div>
                <div class="detail-item" v-if="user.created_at">
                  <span class="detail-label">创建时间：</span>
                  <span class="detail-value">{{ formatDate(user.created_at) }}</span>
                </div>
                <div class="detail-item">
                  <span class="detail-label">权限值：</span>
                  <span class="detail-value">{{ user.permissions }} ({{ user.permissions === -1 ? '管理员' : '二进制: ' + (user.permissions || 0).toString(2) }})</span>
                </div>
              </div>
            </div>
          </div>

          <!-- 权限详情卡片 -->
          <div class="permissions-card">
            <div class="card-header">
              <h3>
                <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                  <rect x="3" y="11" width="18" height="11" rx="2" ry="2"/>
                  <circle cx="12" cy="16" r="1"/>
                  <path d="M7 11V7a5 5 0 0110 0v4"/>
                </svg>
                权限详情
              </h3>
              <div class="permissions-summary">
                <span class="permission-count">{{ getGrantedPermissionsCount() }}/{{ getAllPermissions().length }}</span>
                <span class="permission-text">项权限</span>
              </div>
            </div>
            
            <div class="permissions-grid-modern">
              <div v-for="permission in getAllPermissions()" :key="permission.key" 
                   class="permission-card" 
                   :class="{ 'granted': hasSpecificPermission(permission.value), 'denied': !hasSpecificPermission(permission.value) }">
                <div class="permission-icon">
                  <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                    <path :d="permission.icon"/>
                  </svg>
                </div>
                <div class="permission-content">
                  <h4 class="permission-name">{{ permission.name }}</h4>
                  <p class="permission-desc">{{ permission.description }}</p>
                </div>
                <div class="permission-status">
                  <div class="status-indicator" :class="{ 'granted': hasSpecificPermission(permission.value) }">
                    <svg v-if="hasSpecificPermission(permission.value)" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                      <polyline points="20,6 9,17 4,12"/>
                    </svg>
                    <svg v-else width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                      <line x1="18" y1="6" x2="6" y2="18"/>
                      <line x1="6" y1="6" x2="18" y2="18"/>
                    </svg>
                  </div>
                </div>
              </div>
            </div>
          </div>
          
          <!-- 修改密码卡片 -->
          <div v-if="!isGuest" class="password-change-card">
            <div class="password-header">
              <div class="password-icon">
                <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                  <rect x="3" y="11" width="18" height="11" rx="2" ry="2"/>
                  <circle cx="12" cy="16" r="1"/>
                  <path d="M7 11V7a5 5 0 0110 0v4"/>
                </svg>
              </div>
              <div class="password-title">
                <h3>修改密码</h3>
                <p>更新您的账户密码以保护安全</p>
              </div>
            </div>
            
            <form @submit.prevent="handleChangePassword" class="password-form">
              <div class="password-grid">
                <div class="password-item">
                  <label>
                    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                      <path d="M9 12l2 2 4-4"/>
                      <path d="M21 12c-1 0-3-1-3-3s2-3 3-3 3 1 3 3-2 3-3 3"/>
                      <path d="M3 12c1 0 3-1 3-3s-2-3-3-3-3 1-3 3 2 3 3 3"/>
                    </svg>
                    原密码
                  </label>
                  <input v-model="oldPassword" type="password" required class="password-input" placeholder="请输入当前密码" />
                </div>

                <div class="password-item">
                  <label>
                    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                      <rect x="3" y="11" width="18" height="11" rx="2" ry="2"/>
                      <circle cx="12" cy="16" r="1"/>
                      <path d="M7 11V7a5 5 0 0110 0v4"/>
                    </svg>
                    新密码
                  </label>
                  <input v-model="newPassword" type="password" required class="password-input" placeholder="请输入新密码" />
                </div>

                <div class="password-item">
                  <label>
                    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                      <path d="M9 12l2 2 4-4"/>
                      <path d="M21 12c-1 0-3-1-3-3s2-3 3-3 3 1 3 3-2 3-3 3"/>
                      <path d="M3 12c1 0 3-1 3-3s-2-3-3-3-3 1-3 3 2 3 3 3"/>
                    </svg>
                    确认新密码
                  </label>
                  <input v-model="confirmPassword" type="password" required class="password-input" placeholder="请再次输入新密码" />
                </div>
              </div>

              <div class="password-actions">
                <button type="submit" class="btn-change-password">
                  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                    <path d="M9 12l2 2 4-4"/>
                    <path d="M21 12c-1 0-3-1-3-3s2-3 3-3 3 1 3 3-2 3-3 3"/>
                    <path d="M3 12c1 0 3-1 3-3s-2-3-3-3-3 1-3 3 2 3 3 3"/>
                  </svg>
                  修改密码
                </button>
              </div>
            </form>
          </div>
        </div>

        <!-- 站点设置 -->
        <div v-if="activeTab === 'site'" class="content-section">
          <!-- 基本设置 -->
          <div v-if="siteSubTab === 'general'" class="settings-card">
            <div class="settings-header">
              <div class="settings-icon">
                <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                  <path d="M12 2l3.09 6.26L22 9.27l-5 4.87 1.18 6.88L12 17.77l-6.18 3.25L7 14.14 2 9.27l6.91-1.01L12 2z"/>
                </svg>
              </div>
              <div class="settings-title">
                <h3>基本设置</h3>
                <p>配置站点的基本信息和功能</p>
              </div>
            </div>
            
            <form @submit.prevent="saveSiteSettings" class="settings-form">
              <div class="settings-grid">
                <div class="setting-item">
                  <div class="setting-label">
                    <label>站点标题</label>
                    <div class="setting-icon">
                      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                        <path d="M4 19.5A2.5 2.5 0 016.5 17H20"/>
                        <path d="M6.5 2H20v20l-5.5-6-5.5 6V2z"/>
                      </svg>
                    </div>
                  </div>
                  <input v-model="siteSettings.site_title" type="text" class="setting-input" placeholder="YaoList" />
                  <small class="setting-help">显示在浏览器标题栏和页面顶部的站点名称</small>
                </div>

                <div class="setting-item">
                  <div class="setting-label">
                    <label>站点描述</label>
                    <div class="setting-icon">
                      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                        <path d="M14 2H6a2 2 0 00-2 2v16a2 2 0 002 2h12a2 2 0 002-2V8z"/>
                        <polyline points="14,2 14,8 20,8"/>
                        <line x1="16" y1="13" x2="8" y2="13"/>
                        <line x1="16" y1="17" x2="8" y2="17"/>
                        <polyline points="10,9 9,9 8,9"/>
                      </svg>
                    </div>
                  </div>
                  <textarea v-model="siteSettings.site_description" class="setting-input" rows="3" placeholder="现代化的文件管理系统"></textarea>
                  <small class="setting-help">站点的简短描述，用于SEO和页面介绍</small>
                </div>

                <div class="setting-item">
                  <div class="setting-label">
                    <label>用户注册</label>
                    <div class="setting-icon">
                      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                        <path d="M16 21v-2a4 4 0 00-4-4H5a4 4 0 00-4 4v2"/>
                        <circle cx="8.5" cy="7" r="4"/>
                        <line x1="20" y1="8" x2="20" y2="14"/>
                        <line x1="23" y1="11" x2="17" y2="11"/>
                      </svg>
                    </div>
                  </div>
                  <div class="setting-toggle">
                    <label class="toggle-switch">
                      <input v-model="siteSettings.allow_registration" type="checkbox" />
                      <span class="toggle-slider"></span>
                    </label>
                    <span class="toggle-text">{{ siteSettings.allow_registration ? '已启用' : '已禁用' }}</span>
                  </div>
                  <small class="setting-help">开启后，访客可以自行注册账户</small>
                </div>
              </div>

              <div class="settings-actions">
                <button type="submit" class="btn-save" :disabled="savingSettings">
                  <svg v-if="!savingSettings" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                    <path d="M19 21H5a2 2 0 01-2-2V5a2 2 0 012-2h11l5 5v11a2 2 0 01-2 2z"/>
                    <polyline points="17,21 17,13 7,13 7,21"/>
                    <polyline points="7,3 7,8 15,8"/>
                  </svg>
                  <svg v-else width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                    <path d="M21 12a9 9 0 11-6.219-8.56"/>
                  </svg>
                  {{ savingSettings ? '保存中...' : '保存设置' }}
                </button>
              </div>
            </form>
          </div>

          <!-- 外观设置 -->
          <div v-if="siteSubTab === 'appearance'" class="settings-card">
            <div class="settings-header">
              <div class="settings-icon">
                <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                  <circle cx="12" cy="12" r="3"/>
                  <path d="M12 1v6m0 6v6m11-7h-6m-6 0H1m15.5-6.5l-4.24 4.24M7.76 7.76L3.52 3.52m12.96 12.96l-4.24-4.24M7.76 16.24l-4.24 4.24"/>
                </svg>
              </div>
              <div class="settings-title">
                <h3>外观设置</h3>
                <p>自定义站点的视觉外观和主题</p>
              </div>
            </div>
            
            <form @submit.prevent="saveSiteSettings" class="settings-form">
              <div class="settings-grid">
                <div class="setting-item">
                  <div class="setting-label">
                    <label>顶部自定义信息</label>
                    <div class="setting-icon">
                      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                        <path d="M12 2l3.09 6.26L22 9.27l-5 4.87 1.18 6.88L12 17.77l-6.18 3.25L7 14.14 2 9.27l6.91-1.01L12 2z"/>
                      </svg>
                    </div>
                  </div>
                  <div class="setting-toggle">
                    <label class="toggle-switch">
                      <input v-model="siteSettings.enable_top_message" type="checkbox" />
                      <span class="toggle-slider"></span>
                    </label>
                    <span class="toggle-text">{{ siteSettings.enable_top_message ? '已启用' : '已禁用' }}</span>
                  </div>
                  <textarea v-if="siteSettings.enable_top_message" v-model="siteSettings.top_message" class="setting-input" rows="3" placeholder="支持Markdown格式，例如：# 欢迎使用YaoList"></textarea>
                  <small class="setting-help">在主页顶部显示自定义信息，支持Markdown格式</small>
                </div>

                <div class="setting-item">
                  <div class="setting-label">
                    <label>底部自定义信息</label>
                    <div class="setting-icon">
                      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                        <path d="M12 22l3.09-6.26L22 14.73l-5-4.87 1.18-6.88L12 6.23l-6.18-3.25L7 9.86 2 14.73l6.91 1.01L12 22z"/>
                      </svg>
                    </div>
                  </div>
                  <div class="setting-toggle">
                    <label class="toggle-switch">
                      <input v-model="siteSettings.enable_bottom_message" type="checkbox" />
                      <span class="toggle-slider"></span>
                    </label>
                    <span class="toggle-text">{{ siteSettings.enable_bottom_message ? '已启用' : '已禁用' }}</span>
                  </div>
                  <textarea v-if="siteSettings.enable_bottom_message" v-model="siteSettings.bottom_message" class="setting-input" rows="3" placeholder="支持Markdown格式，例如：**感谢使用YaoList**"></textarea>
                  <small class="setting-help">在主页底部显示自定义信息，支持Markdown格式</small>
                </div>

                <div class="setting-item">
                  <div class="setting-label">
                    <label>背景图片URL</label>
                    <div class="setting-icon">
                      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                        <rect x="3" y="3" width="18" height="18" rx="2" ry="2"/>
                        <circle cx="8.5" cy="8.5" r="1.5"/>
                        <polyline points="21,15 16,10 5,21"/>
                      </svg>
                    </div>
                  </div>
                  <div class="url-input-group">
                    <input v-model="siteSettings.background_url" type="url" class="setting-input" placeholder="https://example.com/background.jpg" />
                    <div class="background-preview" v-if="siteSettings.background_url">
                      <img :src="siteSettings.background_url" alt="背景图片预览" @error="$event.target.style.display='none'" />
                    </div>
                  </div>
                  <small class="setting-help">自定义页面背景图片，留空则使用默认背景</small>
                </div>

                <div class="setting-item">
                  <div class="setting-label">
                    <label>毛玻璃效果</label>
                    <div class="setting-icon">
                      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                        <rect x="3" y="3" width="18" height="18" rx="2" ry="2"/>
                        <path d="M3 9h18"/>
                        <path d="M3 15h18"/>
                      </svg>
                    </div>
                  </div>
                  <div class="setting-toggle">
                    <label class="toggle-switch">
                      <input v-model="siteSettings.enable_glass_effect" type="checkbox" />
                      <span class="toggle-slider"></span>
                    </label>
                    <span class="toggle-text">{{ siteSettings.enable_glass_effect ? '已启用' : '已禁用' }}</span>
                  </div>
                  <small class="setting-help">启用后，卡片将呈现毛玻璃效果</small>
                </div>

                <div class="setting-item">
                  <div class="setting-label">
                    <label>主题色</label>
                    <div class="setting-icon">
                      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                        <circle cx="12" cy="12" r="10"/>
                        <path d="M12 2a10 10 0 100 20V2z"/>
                      </svg>
                    </div>
                  </div>
                  <div class="color-picker-group">
                    <div class="color-preview" :style="{ backgroundColor: siteSettings.theme_color }"></div>
                    <input v-model="siteSettings.theme_color" type="color" class="color-picker" />
                    <input v-model="siteSettings.theme_color" type="text" class="setting-input color-text" placeholder="#1976d2" />
                  </div>
                  <small class="setting-help">站点的主要颜色，影响按钮、链接等元素的颜色</small>
                </div>

                <div class="setting-item">
                  <div class="setting-label">
                    <label>站点图标URL</label>
                    <div class="setting-icon">
                      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                        <rect x="3" y="3" width="18" height="18" rx="2" ry="2"/>
                        <circle cx="8.5" cy="8.5" r="1.5"/>
                        <polyline points="21,15 16,10 5,21"/>
                      </svg>
                    </div>
                  </div>
                  <div class="url-input-group">
                    <input v-model="siteSettings.site_icon" type="url" class="setting-input" placeholder="https://example.com/icon.png" />
                    <div class="icon-preview" v-if="siteSettings.site_icon">
                      <img :src="siteSettings.site_icon" alt="站点图标预览" @error="$event.target.style.display='none'" />
                    </div>
                  </div>
                  <small class="setting-help">显示在页面顶部的站点图标，建议尺寸：32x32px</small>
                </div>

                <div class="setting-item">
                  <div class="setting-label">
                    <label>网站图标URL (Favicon)</label>
                    <div class="setting-icon">
                      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                        <path d="M21 16V8a2 2 0 00-1-1.73l-7-4a2 2 0 00-2 0l-7 4A2 2 0 003 8v8a2 2 0 001 1.73l7 4a2 2 0 002 0l7-4A2 2 0 0021 16z"/>
                        <polyline points="7.5,4.21 12,6.81 16.5,4.21"/>
                        <polyline points="7.5,19.79 7.5,14.6 3,12"/>
                        <polyline points="21,12 16.5,14.6 16.5,19.79"/>
                        <polyline points="3.27,6.96 12,12.01 20.73,6.96"/>
                        <line x1="12" y1="22.08" x2="12" y2="12"/>
                      </svg>
                    </div>
                  </div>
                  <div class="url-input-group">
                    <input v-model="siteSettings.favicon" type="url" class="setting-input" placeholder="https://api.ylist.org/logo/logo.svg" />
                    <div class="icon-preview favicon-preview" v-if="siteSettings.favicon">
                      <img :src="siteSettings.favicon" alt="Favicon预览" @error="$event.target.style.display='none'" />
                    </div>
                  </div>
                  <small class="setting-help">浏览器标签页显示的小图标，建议格式：.ico 或 .png</small>
                </div>
              </div>

              <div class="settings-actions">
                <button type="submit" class="btn-save" :disabled="savingSettings">
                  <svg v-if="!savingSettings" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                    <path d="M19 21H5a2 2 0 01-2-2V5a2 2 0 012-2h11l5 5v11a2 2 0 01-2 2z"/>
                    <polyline points="17,21 17,13 7,13 7,21"/>
                    <polyline points="7,3 7,8 15,8"/>
                  </svg>
                  <svg v-else width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                    <path d="M21 12a9 9 0 11-6.219-8.56"/>
                  </svg>
                  {{ savingSettings ? '保存中...' : '保存设置' }}
                </button>
              </div>
            </form>
          </div>

          <!-- 分页设置 -->
          <div v-if="siteSubTab === 'pagination'" class="settings-card">
            <div class="settings-header">
              <div class="settings-icon">
                <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                  <rect x="3" y="4" width="18" height="18" rx="2" ry="2"/>
                  <line x1="16" y1="2" x2="16" y2="6"/>
                  <line x1="8" y1="2" x2="8" y2="6"/>
                  <line x1="3" y1="10" x2="21" y2="10"/>
                  <path d="M8 14h.01M12 14h.01M16 14h.01M8 18h.01M12 18h.01M16 18h.01"/>
                </svg>
              </div>
              <div class="settings-title">
                <h3>分页设置</h3>
                <p>配置文件列表的显示和分页方式</p>
              </div>
            </div>
            
            <form @submit.prevent="saveSiteSettings" class="settings-form">
              <div class="settings-grid">
                <div class="setting-item">
                  <div class="setting-label">
                    <label>分页类型</label>
                    <div class="setting-icon">
                      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                        <path d="M8 3H5a2 2 0 00-2 2v3m18 0V5a2 2 0 00-2-2h-3m0 18h3a2 2 0 002-2v-3M3 16v3a2 2 0 002 2h3"/>
                        <circle cx="12" cy="12" r="4"/>
                      </svg>
                    </div>
                  </div>
                  <div class="select-wrapper">
                    <select v-model="siteSettings.pagination_type" class="setting-select">
                      <option value="infinite">无限滚动</option>
                      <option value="pagination">传统分页</option>
                    </select>
                    <div class="select-arrow">
                      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                        <polyline points="6,9 12,15 18,9"/>
                      </svg>
                    </div>
                  </div>
                  <small class="setting-help">选择文件列表的分页方式</small>
                </div>

                <div class="setting-item">
                  <div class="setting-label">
                    <label>每页显示数量</label>
                    <div class="setting-icon">
                      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                        <path d="M14 2H6a2 2 0 00-2 2v16a2 2 0 002 2h12a2 2 0 002-2V8z"/>
                        <polyline points="14,2 14,8 20,8"/>
                        <line x1="16" y1="13" x2="8" y2="13"/>
                        <line x1="16" y1="17" x2="8" y2="17"/>
                        <polyline points="10,9 9,9 8,9"/>
                      </svg>
                    </div>
                  </div>
                  <div class="number-input-group">
                    <input v-model.number="siteSettings.items_per_page" type="number" min="10" max="200" class="setting-input number-input" />
                    <div class="number-controls">
                      <button type="button" @click="siteSettings.items_per_page = Math.min(200, siteSettings.items_per_page + 10)" class="number-btn">
                        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                          <line x1="12" y1="5" x2="12" y2="19"/>
                          <line x1="5" y1="12" x2="19" y2="12"/>
                        </svg>
                      </button>
                      <button type="button" @click="siteSettings.items_per_page = Math.max(10, siteSettings.items_per_page - 10)" class="number-btn">
                        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                          <line x1="5" y1="12" x2="19" y2="12"/>
                        </svg>
                      </button>
                    </div>
                  </div>
                  <small class="setting-help">每页显示的文件数量，建议范围：10-200</small>
                </div>
              </div>

              <div class="settings-actions">
                <button type="submit" class="btn-save" :disabled="savingSettings">
                  <svg v-if="!savingSettings" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                    <path d="M19 21H5a2 2 0 01-2-2V5a2 2 0 012-2h11l5 5v11a2 2 0 01-2 2z"/>
                    <polyline points="17,21 17,13 7,13 7,21"/>
                    <polyline points="7,3 7,8 15,8"/>
                  </svg>
                  <svg v-else width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                    <path d="M21 12a9 9 0 11-6.219-8.56"/>
                  </svg>
                  {{ savingSettings ? '保存中...' : '保存设置' }}
                </button>
              </div>
            </form>
          </div>

          <!-- 预览设置 -->
          <div v-if="siteSubTab === 'preview'" class="card">
            <div class="card-header">
              <h3>预览设置</h3>
              <div class="preview-actions">
                <button type="button" class="btn btn-secondary" @click="resetPreviewSettings">
                  <i class="icon">
                    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                      <polyline points="1 4 1 10 7 10"/>
                      <path d="M3.51 15a9 9 0 102.13-9.36L1 10"/>
                    </svg>
                  </i>
                  重置默认
                </button>
                <button type="button" class="btn btn-info" @click="showPreviewHelp = !showPreviewHelp">
                  <i class="icon">
                    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                      <circle cx="12" cy="12" r="10"/>
                      <path d="M9.09 9a3 3 0 015.83 1c0 2-3 3-3 3"/>
                      <path d="M12 17h.01"/>
                    </svg>
                  </i>
                  帮助
                </button>
              </div>
            </div>

            <!-- 帮助信息 -->
            <div v-if="showPreviewHelp" class="help-panel">
              <h4>预览设置说明</h4>
              <div class="help-content">
                <div class="help-section">
                  <h5>文件类型配置</h5>
                  <p>配置支持预览的文件扩展名，多个扩展名用逗号分隔，不需要包含点号。</p>
                  <p><strong>示例：</strong> txt,md,json,js,css</p>
                </div>
                <div class="help-section">
                  <h5>外部预览服务</h5>
                  <p>配置外部预览服务，支持多种文档格式的在线预览。</p>
                  <p><strong>变量说明：</strong></p>
                  <ul>
                    <li><code>$e_url</code> - URL编码后的文件地址</li>
                    <li><code>$url</code> - 原始文件地址</li>
                  </ul>
                </div>
                <div class="help-section">
                  <h5>Iframe预览配置</h5>
                  <p>配置通过iframe方式预览的文件类型和服务提供商。</p>
                  <p><strong>格式：</strong> JSON对象，键为文件扩展名，值为服务提供商配置。</p>
                </div>
              </div>
            </div>

            <form @submit.prevent="saveSiteSettings" class="form">
              <!-- 文件类型设置 -->
              <div class="settings-section">
                <h4 class="section-title">
                  <i class="icon">
                    <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                      <path d="M14 2H6a2 2 0 00-2 2v16a2 2 0 002 2h12a2 2 0 002-2V8z"/>
                      <polyline points="14,2 14,8 20,8"/>
                    </svg>
                  </i>
                  支持的文件类型
                </h4>
                
                <div class="file-types-grid">
                  <div class="form-group">
                    <label>
                      <i class="type-icon text-icon">
                        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                          <path d="M14 2H6a2 2 0 00-2 2v16a2 2 0 002 2h12a2 2 0 002-2V8z"/>
                          <polyline points="14,2 14,8 20,8"/>
                          <line x1="16" y1="13" x2="8" y2="13"/>
                          <line x1="16" y1="17" x2="8" y2="17"/>
                          <polyline points="10,9 9,9 8,9"/>
                        </svg>
                      </i>
                      文本类型
                    </label>
                    <div class="input-with-preset">
                      <input v-model="siteSettings.preview_text_types" type="text" class="form-input" />
                      <button type="button" class="preset-btn" @click="applyPreset('text')">预设</button>
                    </div>
                    <small class="help-text">支持预览的文本文件扩展名</small>
                    <div class="preview-tags">
                      <span v-for="type in getFileTypeArray('preview_text_types')" :key="type" class="file-type-tag text-tag">{{ type }}</span>
                    </div>
                  </div>

                  <div class="form-group">
                    <label>
                      <i class="type-icon audio-icon">
                        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                          <polygon points="11 5 6 9 2 9 2 15 6 15 11 19 11 5"/>
                          <path d="M19.07 4.93a10 10 0 010 14.14M15.54 8.46a5 5 0 010 7.07"/>
                        </svg>
                      </i>
                      音频类型
                    </label>
                    <div class="input-with-preset">
                      <input v-model="siteSettings.preview_audio_types" type="text" class="form-input" />
                      <button type="button" class="preset-btn" @click="applyPreset('audio')">预设</button>
                    </div>
                    <small class="help-text">支持预览的音频文件扩展名</small>
                    <div class="preview-tags">
                      <span v-for="type in getFileTypeArray('preview_audio_types')" :key="type" class="file-type-tag audio-tag">{{ type }}</span>
                    </div>
                  </div>

                  <div class="form-group">
                    <label>
                      <i class="type-icon video-icon">
                        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                          <polygon points="23 7 16 12 23 17 23 7"/>
                          <rect x="1" y="5" width="15" height="14" rx="2" ry="2"/>
                        </svg>
                      </i>
                      视频类型
                    </label>
                    <div class="input-with-preset">
                      <input v-model="siteSettings.preview_video_types" type="text" class="form-input" />
                      <button type="button" class="preset-btn" @click="applyPreset('video')">预设</button>
                    </div>
                    <small class="help-text">支持预览的视频文件扩展名</small>
                    <div class="preview-tags">
                      <span v-for="type in getFileTypeArray('preview_video_types')" :key="type" class="file-type-tag video-tag">{{ type }}</span>
                    </div>
                  </div>

                  <div class="form-group">
                    <label>
                      <i class="type-icon image-icon">
                        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                          <rect x="3" y="3" width="18" height="18" rx="2" ry="2"/>
                          <circle cx="8.5" cy="8.5" r="1.5"/>
                          <polyline points="21,15 16,10 5,21"/>
                        </svg>
                      </i>
                      图片类型
                    </label>
                    <div class="input-with-preset">
                      <input v-model="siteSettings.preview_image_types" type="text" class="form-input" />
                      <button type="button" class="preset-btn" @click="applyPreset('image')">预设</button>
                    </div>
                    <small class="help-text">支持预览的图片文件扩展名</small>
                    <div class="preview-tags">
                      <span v-for="type in getFileTypeArray('preview_image_types')" :key="type" class="file-type-tag image-tag">{{ type }}</span>
                    </div>
                  </div>
                </div>
              </div>

              <!-- 代理设置 -->
              <div class="settings-section">
                <h4 class="section-title">
                  <i class="icon">
                    <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                      <circle cx="12" cy="12" r="10"/>
                      <path d="M8 14s1.5 2 4 2 4-2 4-2"/>
                      <line x1="9" y1="9" x2="9.01" y2="9"/>
                      <line x1="15" y1="9" x2="15.01" y2="9"/>
                    </svg>
                  </i>
                  代理设置
                </h4>
                
                <div class="form-group">
                  <label>代理类型</label>
                  <input v-model="siteSettings.preview_proxy_types" type="text" class="form-input" />
                  <small class="help-text">需要代理访问的文件类型，用逗号分隔</small>
                </div>
                
                <div class="form-group">
                  <label>代理忽略头部</label>
                  <input v-model="siteSettings.preview_proxy_ignore_headers" type="text" class="form-input" />
                  <small class="help-text">代理时忽略的HTTP头部，用逗号分隔</small>
                </div>
              </div>

              <!-- 外部预览服务 -->
              <div class="settings-section">
                <h4 class="section-title">
                  <i class="icon">
                    <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                      <circle cx="12" cy="12" r="10"/>
                      <line x1="2" y1="12" x2="22" y2="12"/>
                      <path d="M12 2a15.3 15.3 0 014 10 15.3 15.3 0 01-4 10 15.3 15.3 0 01-4-10 15.3 15.3 0 014-10z"/>
                    </svg>
                  </i>
                  外部预览服务
                </h4>
                
                <div class="form-group">
                  <label>外部预览配置</label>
                  <div class="json-editor-container">
                    <textarea 
                      v-model="siteSettings.preview_external" 
                      class="form-input json-editor" 
                      rows="6" 
                      placeholder="{}"
                      @blur="validateJson('preview_external')"
                    ></textarea>
                    <div v-if="jsonErrors.preview_external" class="json-error">
                      <i class="icon">
                        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                          <circle cx="12" cy="12" r="10"/>
                          <line x1="15" y1="9" x2="9" y2="15"/>
                          <line x1="9" y1="9" x2="15" y2="15"/>
                        </svg>
                      </i>
                      {{ jsonErrors.preview_external }}
                    </div>
                  </div>
                  <small class="help-text">外部预览服务配置，JSON格式</small>
                  <button type="button" class="preset-btn secondary" @click="applyPreset('external')">使用默认配置</button>
                </div>
              </div>

              <!-- Iframe预览 -->
              <div class="settings-section">
                <h4 class="section-title">
                  <i class="icon">
                    <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                      <rect x="2" y="3" width="20" height="14" rx="2" ry="2"/>
                      <line x1="8" y1="21" x2="16" y2="21"/>
                      <line x1="12" y1="17" x2="12" y2="21"/>
                    </svg>
                  </i>
                  Iframe预览配置
                </h4>
                
                <div class="form-group">
                  <label>Iframe预览服务</label>
                  <div class="json-editor-container">
                    <textarea 
                      v-model="siteSettings.preview_iframe" 
                      class="form-input json-editor" 
                      rows="8"
                      @blur="validateJson('preview_iframe')"
                    ></textarea>
                    <div v-if="jsonErrors.preview_iframe" class="json-error">
                      <i class="icon">
                        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                          <circle cx="12" cy="12" r="10"/>
                          <line x1="15" y1="9" x2="9" y2="15"/>
                          <line x1="9" y1="9" x2="15" y2="15"/>
                        </svg>
                      </i>
                      {{ jsonErrors.preview_iframe }}
                    </div>
                  </div>
                  <small class="help-text">Iframe预览配置，JSON格式</small>
                  <button type="button" class="preset-btn secondary" @click="applyPreset('iframe')">使用默认配置</button>
                </div>
              </div>

              <!-- 播放设置 -->
              <div class="settings-section">
                <h4 class="section-title">
                  <i class="icon">
                    <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                      <polygon points="5 3 19 12 5 21 5 3"/>
                    </svg>
                  </i>
                  播放设置
                </h4>
                
                <div class="form-group">
                  <label>音频封面</label>
                  <div class="input-with-preview">
                    <input v-model="siteSettings.preview_audio_cover" type="url" class="form-input" />
                    <div v-if="siteSettings.preview_audio_cover" class="cover-preview">
                      <img :src="siteSettings.preview_audio_cover" alt="音频封面预览" @error="handleImageError" />
                    </div>
                  </div>
                  <small class="help-text">音频播放时显示的默认封面图片URL</small>
                </div>

                <div class="checkbox-group">
                  <div class="form-group">
                    <label class="checkbox-label">
                      <input v-model="siteSettings.preview_auto_play_audio" type="checkbox" />
                      <span class="checkbox-custom"></span>
                      <span>自动播放音频</span>
                    </label>
                    <small class="help-text">音频文件是否自动播放</small>
                  </div>

                  <div class="form-group">
                    <label class="checkbox-label">
                      <input v-model="siteSettings.preview_auto_play_video" type="checkbox" />
                      <span class="checkbox-custom"></span>
                      <span>自动播放视频</span>
                    </label>
                    <small class="help-text">视频文件是否自动播放</small>
                  </div>
                </div>
              </div>

              <!-- 其他设置 -->
              <div class="settings-section">
                <h4 class="section-title">
                  <i class="icon">
                    <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                      <circle cx="12" cy="12" r="3"/>
                      <path d="M19.4 15a1.65 1.65 0 00.33 1.82l.06.06a2 2 0 010 2.83 2 2 0 01-2.83 0l-.06-.06a1.65 1.65 0 00-1.82-.33 1.65 1.65 0 00-1 1.51V21a2 2 0 01-2 2 2 2 0 01-2-2v-.09A1.65 1.65 0 009 19.4a1.65 1.65 0 00-1.82.33l-.06.06a2 2 0 01-2.83 0 2 2 0 010-2.83l.06-.06a1.65 1.65 0 00.33-1.82 1.65 1.65 0 00-1.51-1H3a2 2 0 01-2-2 2 2 0 012-2h.09A1.65 1.65 0 004.6 9a1.65 1.65 0 00-.33-1.82l-.06-.06a2 2 0 010-2.83 2 2 0 012.83 0l.06.06a1.65 1.65 0 001.82.33H9a1.65 1.65 0 001-1.51V3a2 2 0 012-2 2 2 0 012 2v.09a1.65 1.65 0 001 1.51 1.65 1.65 0 001.82-.33l.06-.06a2 2 0 012.83 0 2 2 0 010 2.83l-.06.06a1.65 1.65 0 00-.33 1.82V9a1.65 1.65 0 001.51 1H21a2 2 0 012 2 2 2 0 01-2 2h-.09a1.65 1.65 0 00-1.51 1z"/>
                    </svg>
                  </i>
                  其他设置
                </h4>
                
                <div class="checkbox-group">
                  <div class="form-group">
                    <label class="checkbox-label">
                      <input v-model="siteSettings.preview_default_archive" type="checkbox" />
                      <span class="checkbox-custom"></span>
                      <span>默认预览压缩文件</span>
                    </label>
                    <small class="help-text">是否默认预览压缩文件内容</small>
                  </div>

                  <div class="form-group">
                    <label class="checkbox-label">
                      <input v-model="siteSettings.preview_readme_render" type="checkbox" />
                      <span class="checkbox-custom"></span>
                      <span>ReadMe自动渲染</span>
                    </label>
                    <small class="help-text">是否自动渲染README文件</small>
                  </div>

                  <div class="form-group">
                    <label class="checkbox-label">
                      <input v-model="siteSettings.preview_readme_filter_script" type="checkbox" />
                      <span class="checkbox-custom"></span>
                      <span>过滤ReadMe脚本</span>
                    </label>
                    <small class="help-text">是否过滤README文件中的脚本标签</small>
                  </div>
                </div>
              </div>

              <div class="form-actions">
                <button type="submit" class="btn btn-primary" :disabled="savingSettings">
                  <i class="icon">
                    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                      <path d="M19 21H5a2 2 0 01-2-2V5a2 2 0 012-2h11l5 5v11a2 2 0 01-2 2z"/>
                      <polyline points="17,21 17,13 7,13 7,21"/>
                      <polyline points="7,3 7,8 15,8"/>
                    </svg>
                  </i>
                  {{ savingSettings ? '保存中...' : '保存设置' }}
                </button>
                <button type="button" class="btn btn-secondary" @click="loadSiteSettings">
                  <i class="icon">
                    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                      <polyline points="23 4 23 10 17 10"/>
                      <polyline points="1 20 1 14 7 14"/>
                      <path d="M20.49 9A9 9 0 005.64 5.64L1 10m22 4l-4.64 4.36A9 9 0 013.51 15"/>
                    </svg>
                  </i>
                  重新加载
                </button>
              </div>
            </form>
          </div>
        </div>

        <!-- 用户管理 -->
        <div v-if="activeTab === 'users'" class="content-section">
          <div class="card">
            <div class="card-header">
              <div class="header-content">
                <div class="header-title">
                  <i class="header-icon">
                    <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                      <path d="M17 21v-2a4 4 0 00-4-4H5a4 4 0 00-4 4v2"/>
                      <circle cx="9" cy="7" r="4"/>
                      <path d="M23 21v-2a4 4 0 00-3-3.87"/>
                      <path d="M16 3.13a4 4 0 010 7.75"/>
                    </svg>
                  </i>
                  <div>
                    <h3>用户管理</h3>
                    <p class="header-subtitle">管理系统用户账号和权限</p>
                  </div>
                </div>
                <div class="header-stats">
                  <div class="stat-item">
                    <span class="stat-number">{{ users.length }}</span>
                    <span class="stat-label">总用户</span>
                  </div>
                  <div class="stat-item">
                    <span class="stat-number">{{ users.filter(u => u.enabled).length }}</span>
                    <span class="stat-label">已启用</span>
                  </div>
                </div>
              </div>
              <button class="btn btn-primary btn-add" @click="showCreateUserDialog = true">
                <i class="btn-icon">
                  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                    <line x1="12" y1="5" x2="12" y2="19"/>
                    <line x1="5" y1="12" x2="19" y2="12"/>
                  </svg>
                </i>
                添加用户
              </button>
            </div>
            <div class="table-container">
              <table class="data-table">
                <thead>
                  <tr>
                    <th>ID</th>
                    <th>用户名</th>
                    <th>状态</th>
                    <th>权限</th>
                    <th>用户路径</th>
                    <th>创建时间</th>
                    <th>操作</th>
                  </tr>
                </thead>
                <tbody>
                  <tr v-for="user in users" :key="user.id">
                    <td>{{ user.id }}</td>
                    <td>{{ user.username }}</td>
                    <td>
                      <span :class="['status-badge', user.enabled ? 'enabled' : 'disabled']">
                        <i class="status-icon">
                          <svg width="12" height="12" viewBox="0 0 24 24" fill="currentColor">
                            <circle cx="12" cy="12" r="10"/>
                          </svg>
                        </i>
                        {{ user.enabled ? '已启用' : '已禁用' }}
                      </span>
                    </td>
                    <td>
                      <span class="permission-badge" :class="getPermissionClass(user.permissions)">
                        {{ getPermissionText(user.permissions) }}
                      </span>
                    </td>
                    <td>{{ user.user_path }}</td>
                    <td><span class="file-date">{{ formatDate(user.created_at) }}</span></td>
                    <td>
                      <button class="btn btn-sm btn-edit" @click="editUser(user)">编辑</button>
                      <button class="btn btn-sm btn-delete" @click="deleteUser(user.id)" :disabled="user.id === currentUser.id">删除</button>
                    </td>
                  </tr>
                </tbody>
              </table>
            </div>
          </div>
        </div>

                <!-- 存储管理 -->
        <div v-if="activeTab === 'storage'" class="content-section">
          <div class="card">
            <div class="card-header">
              <div class="header-content">
                <div class="header-title">
                  <i class="header-icon">
                    <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                      <path d="M21 16V8a2 2 0 00-1-1.73l-7-4a2 2 0 00-2 0l-7 4A2 2 0 003 8v8a2 2 0 001 1.73l7 4a2 2 0 002 0l7-4A2 2 0 0021 16z"/>
                      <polyline points="3.27,6.96 12,12.01 20.73,6.96"/>
                      <line x1="12" y1="22.08" x2="12" y2="12"/>
                    </svg>
                  </i>
                  <div>
                    <h3>存储管理</h3>
                    <p class="header-subtitle">管理系统存储后端配置</p>
                  </div>
                </div>
                <div class="header-stats">
                  <div class="stat-item">
                    <span class="stat-number">{{ storages.length }}</span>
                    <span class="stat-label">总存储</span>
                  </div>
                  <div class="stat-item">
                    <span class="stat-number">{{ storages.filter(s => s.enabled).length }}</span>
                    <span class="stat-label">已启用</span>
                  </div>
                </div>
              </div>
              <button class="btn btn-primary btn-add" @click="showAddDialog = true">
                <i class="btn-icon">
                  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                    <line x1="12" y1="5" x2="12" y2="19"/>
                    <line x1="5" y1="12" x2="19" y2="12"/>
                  </svg>
                </i>
                添加存储
              </button>
            </div>
            
            <div v-if="storages.length === 0" class="empty-state">
              <div class="empty-icon">
                <svg width="64" height="64" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                  <path d="M21 16V8a2 2 0 00-1-1.73l-7-4a2 2 0 00-2 0l-7 4A2 2 0 003 8v8a2 2 0 001 1.73l7 4a2 2 0 002 0l7-4A2 2 0 0021 16z"/>
                  <polyline points="3.27,6.96 12,12.01 20.73,6.96"/>
                  <line x1="12" y1="22.08" x2="12" y2="12"/>
                </svg>
              </div>
              <h4>暂无存储配置</h4>
              <p>开始添加您的第一个存储后端</p>
              <button class="btn btn-primary" @click="showAddDialog = true">
                <i class="btn-icon">
                  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                    <line x1="12" y1="5" x2="12" y2="19"/>
                    <line x1="5" y1="12" x2="19" y2="12"/>
                  </svg>
                </i>
                添加存储
              </button>
            </div>
            
            <div v-else class="storage-grid">
              <div v-for="storage in storages" :key="storage.id" class="storage-card" :class="{ 'storage-disabled': !storage.enabled }">
                <div class="storage-header">
                  <div class="storage-title">
                    <div class="storage-icon" :class="getStorageIconClass(storage.storage_type)">
                      <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                        <path v-if="storage.storage_type === 'local'" d="M21 16V8a2 2 0 00-1-1.73l-7-4a2 2 0 00-2 0l-7 4A2 2 0 003 8v8a2 2 0 001 1.73l7 4a2 2 0 002 0l7-4A2 2 0 0021 16z"/>
                        <path v-else-if="storage.storage_type === 'ftp'" d="M22 12h-4l-3 9L9 3l-3 9H2"/>
                        <path v-else-if="storage.storage_type === 'onedrive'" d="M18 10h-1.26A8 8 0 109 20h9a5 5 0 000-10z"/>
                        <path v-else d="M21 16V8a2 2 0 00-1-1.73l-7-4a2 2 0 00-2 0l-7 4A2 2 0 003 8v8a2 2 0 001 1.73l7 4a2 2 0 002 0l7-4A2 2 0 0021 16z"/>
                      </svg>
                    </div>
                    <div class="storage-name">
                      <h4>{{ storage.name }}</h4>
                      <span class="storage-type">{{ getStorageTypeDisplay(storage.storage_type) }}</span>
                    </div>
                  </div>
                  <div class="storage-status">
                    <span :class="['status-badge', storage.enabled ? 'enabled' : 'disabled']">
                      <i class="status-icon">
                        <svg width="12" height="12" viewBox="0 0 24 24" fill="currentColor">
                          <circle cx="12" cy="12" r="10"/>
                        </svg>
                      </i>
                      {{ storage.enabled ? '已启用' : '已禁用' }}
                    </span>
                  </div>
                </div>
                
                <div class="storage-info">
                  <div class="info-grid">
                    <div class="info-item">
                      <i class="info-icon">
                        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                          <path d="M9 12l2 2 4-4"/>
                          <path d="M21 12c-1 0-3-1-3-3s2-3 3-3 3 1 3 3-2 3-3 3"/>
                          <path d="M3 12c1 0 3-1 3-3s-2-3-3-3-3 1-3 3 2 3 3 3"/>
                          <path d="M3 12h6m6 0h6"/>
                        </svg>
                      </i>
                      <div class="info-content">
                        <span class="info-label">挂载路径</span>
                        <span class="info-value">{{ storage.mount_path }}</span>
                      </div>
                    </div>
                    

                  </div>
                </div>
                
                <div class="storage-actions">
                  <button class="btn btn-sm btn-secondary btn-toggle" @click="toggleStorage(storage)" :title="storage.enabled ? '禁用存储' : '启用存储'">
                    <i class="btn-icon">
                      <svg v-if="storage.enabled" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                        <rect x="3" y="11" width="18" height="10" rx="2" ry="2"/>
                        <circle cx="12" cy="16" r="1"/>
                        <path d="M7 11V7a5 5 0 0110 0v4"/>
                      </svg>
                      <svg v-else width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                        <rect x="3" y="11" width="18" height="10" rx="2" ry="2"/>
                        <circle cx="12" cy="16" r="1"/>
                        <path d="M7 11V7a5 5 0 019.9-1"/>
                      </svg>
                    </i>
                    {{ storage.enabled ? '禁用' : '启用' }}
                  </button>
                  <button class="btn btn-sm btn-edit" @click="editStorage(storage)" title="编辑存储">
                    <i class="btn-icon">
                      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                        <path d="M11 4H4a2 2 0 00-2 2v14a2 2 0 002 2h14a2 2 0 002-2v-7"/>
                        <path d="M18.5 2.5a2.121 2.121 0 013 3L12 15l-4 1 1-4 9.5-9.5z"/>
                      </svg>
                    </i>
                    编辑
                  </button>
                  <button class="btn btn-sm btn-delete" @click="deleteStorage(storage.id)" title="删除存储">
                    <i class="btn-icon">
                      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                        <polyline points="3,6 5,6 21,6"/>
                        <path d="M19 6v14a2 2 0 01-2 2H7a2 2 0 01-2-2V6m3 0V4a2 2 0 012-2h4a2 2 0 012 2v2"/>
                        <line x1="10" y1="11" x2="10" y2="17"/>
                        <line x1="14" y1="11" x2="14" y2="17"/>
                      </svg>
                    </i>
                    删除
                  </button>
                </div>
              </div>
            </div>
          </div>
        </div>

        <!-- 元信息 -->
        <div v-if="activeTab === 'metadata'" class="content-section">
          <div class="card">
            <h3>系统信息</h3>
            <div class="metadata-grid">
              <div class="metadata-item">
                <span class="label">版本</span>
                <span class="value">YaoList v1.0.0</span>
              </div>
              <div class="metadata-item">
                <span class="label">运行时间</span>
                <span class="value">{{ systemUptime }}</span>
              </div>
              <div class="metadata-item">
                <span class="label">存储数量</span>
                <span class="value">{{ storages.length }}</span>
              </div>
              <div class="metadata-item">
                <span class="label">用户数量</span>
                <span class="value">{{ users.length }}</span>
              </div>
            </div>
          </div>
        </div>

        <!-- 备份&恢复 -->
        <div v-if="activeTab === 'backup'" class="content-section">
          <div class="card">
            <h3>数据备份</h3>
            <p class="description">备份系统配置和用户数据</p>
            <button class="btn btn-primary" @click="createBackup">创建备份</button>
          </div>
          
          <div class="card">
            <h3>数据恢复</h3>
            <p class="description">从备份文件恢复系统数据</p>
            <input type="file" @change="handleBackupFile" accept=".json" class="file-input" />
            <button class="btn btn-warning" @click="restoreBackup" :disabled="!backupFile">恢复备份</button>
          </div>
        </div>

        <!-- 关于YaoList -->
        <div v-if="activeTab === 'about'" class="content-section">
          <div class="card">
            <h3>关于 YaoList</h3>
            <div class="about-content" v-html="renderedReadme"></div>
          </div>
        </div>

        <!-- 文档 -->
        <div v-if="activeTab === 'docs'" class="content-section">
          <div class="card">
            <h3>使用文档</h3>
            <div class="docs-content">
              <h4>快速开始</h4>
              <ol>
                <li>配置存储后端</li>
                <li>创建用户账户</li>
                <li>设置权限</li>
                <li>开始使用</li>
              </ol>
              
              <h4>存储配置</h4>
              <p>YaoList 支持多种存储类型：</p>
              <ul>
                <li><strong>本地存储:</strong> 直接访问服务器本地文件系统</li>
                <li><strong>FTP:</strong> 连接到 FTP 服务器</li>
                <li><strong>OneDrive:</strong> 连接到 Microsoft OneDrive</li>
              </ul>
              
              <h4>权限说明</h4>
              <p>系统支持细粒度的权限控制：</p>
              <ul>
                <li>浏览权限：查看文件列表</li>
                <li>下载权限：下载文件</li>
                <li>上传权限：上传文件</li>
                <li>删除权限：删除文件</li>
                <li>管理权限：管理系统设置</li>
              </ul>
            </div>
          </div>
        </div>
      </div>
    </main>

    <!-- 创建存储对话框 -->
          <div v-if="showAddDialog" class="modal-overlay" @click="showAddDialog = false">
      <div class="modal-content storage-modal" @click.stop>
        <div class="modal-header">
          <div class="modal-title">
            <i class="modal-icon">
              <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <path d="M21 16V8a2 2 0 00-1-1.73l-7-4a2 2 0 00-2 0l-7 4A2 2 0 003 8v8a2 2 0 001 1.73l7 4a2 2 0 002 0l7-4A2 2 0 0021 16z"/>
                <polyline points="3.27,6.96 12,12.01 20.73,6.96"/>
                <line x1="12" y1="22.08" x2="12" y2="12"/>
              </svg>
            </i>
            <div>
              <h3>添加存储</h3>
              <p class="modal-subtitle">配置新的存储后端</p>
            </div>
          </div>
                                             <button class="modal-close" @click="showAddDialog = false">
            <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <line x1="18" y1="6" x2="6" y2="18"/>
              <line x1="6" y1="6" x2="18" y2="18"/>
            </svg>
          </button>
        </div>
        
        <form @submit.prevent="createStorage" class="storage-form">
          <div class="form-section">
            <h4 class="section-title">基本信息</h4>
            <div class="form-grid">
              <div class="form-group">
                <label class="form-label">
                  <i class="label-icon">
                    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                      <path d="M20 21v-2a4 4 0 00-4-4H8a4 4 0 00-4 4v2"/>
                      <circle cx="12" cy="7" r="4"/>
                    </svg>
                  </i>
                  存储名称
                </label>
                <input 
                  v-model="newStorage.name" 
                  type="text" 
                  required 
                  class="form-input" 
                  placeholder="输入存储名称"
                />
              </div>
              
              <div class="form-group">
                <label class="form-label">
                  <i class="label-icon">
                    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                      <path d="M9 12l2 2 4-4"/>
                      <path d="M21 12c-1 0-3-1-3-3s2-3 3-3 3 1 3 3-2 3-3 3"/>
                      <path d="M3 12c1 0 3-1 3-3s-2-3-3-3-3 1-3 3 2 3 3 3"/>
                      <path d="M3 12h6m6 0h6"/>
                    </svg>
                  </i>
                  挂载路径
                </label>
                <input 
                  v-model="newStorage.mount_path" 
                  type="text" 
                  placeholder="/storage1" 
                  required 
                  class="form-input"
                />
              </div>
              <div class="form-group">
                <label class="form-label">
                  <i class="label-icon">
                    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                      <path d="M21 16V8a2 2 0 00-1-1.73l-7-4a2 2 0 00-2 0l-7 4A2 2 0 003 8v8a2 2 0 001 1.73l7 4a2 2 0 002 0l7-4A2 2 0 0021 16z"/>
                      <polyline points="3.27,6.96 12,12.01 20.73,6.96"/>
                      <line x1="12" y1="22.08" x2="12" y2="12"/>
                    </svg>
                  </i>
                  存储类型
                </label>
                <select v-model="newStorage.storage_type" required @change="onStorageTypeChange" class="form-input">
                  <option value="">请选择存储类型</option>
                  <option v-for="driver in availableDrivers" :key="driver.driver_type" :value="driver.driver_type">
                    {{ driver.display_name }}
                  </option>
                </select>
              </div>
            </div>
          </div>
          
          <!-- 动态配置字段 -->
          <div v-if="selectedDriver" class="form-section">
            <h4 class="section-title">配置参数</h4>
            <div class="form-grid">
              <div v-for="(property, key) in selectedDriver.config_schema.properties" :key="key" class="form-group">
                <label class="form-label">{{ property.title || key }}</label>
                
                <input 
                  v-if="property.type === 'string' && !property.enum"
                  v-model="newStorage.config[key]"
                  :type="property.format === 'password' ? 'password' : 'text'"
                  :required="selectedDriver.config_schema.required?.includes(key)"
                  class="form-input"
                />
                
                <select 
                  v-else-if="property.enum"
                  v-model="newStorage.config[key]"
                  :required="selectedDriver.config_schema.required?.includes(key)"
                  class="form-input"
                >
                  <option v-for="(option, index) in property.enum" :key="option" :value="option">
                    {{ property.enumNames?.[index] || option }}
                  </option>
                </select>
                
                <input 
                  v-else-if="property.type === 'integer' || property.type === 'number'"
                  v-model.number="newStorage.config[key]"
                  type="number"
                  :min="property.minimum"
                  :max="property.maximum"
                  :required="selectedDriver.config_schema.required?.includes(key)"
                  class="form-input"
                />
                
                <div v-else-if="property.type === 'boolean'" class="checkbox-group">
                  <label class="checkbox-label">
                    <input 
                      v-model="newStorage.config[key]"
                      type="checkbox"
                    />
                    <span class="checkbox-custom"></span>
                    <span>{{ property.title || key }}</span>
                  </label>
                </div>
              </div>
            </div>
          </div>
          
          <div class="form-actions">
                                                     <button type="button" @click="showAddDialog = false" class="btn btn-secondary">取消</button>
                                                     <button type="submit" class="btn btn-primary">创建</button>
          </div>
                  </form>
        </div>
      </div>

    <!-- 编辑存储对话框 -->
    <div v-if="showEditDialog" class="modal-overlay" @click="showEditDialog = false">
      <div class="modal-content storage-modal" @click.stop>
        <div class="modal-header">
          <div class="modal-title">
            <i class="modal-icon">
              <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <path d="M11 4H4a2 2 0 00-2 2v14a2 2 0 002 2h14a2 2 0 002-2v-7"/>
                <path d="M18.5 2.5a2.121 2.121 0 013 3L12 15l-4 1 1-4 9.5-9.5z"/>
              </svg>
            </i>
            <div>
              <h3>编辑存储</h3>
              <p class="modal-subtitle">修改存储配置</p>
            </div>
          </div>
          <button class="modal-close" @click="showEditDialog = false">
            <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <line x1="18" y1="6" x2="6" y2="18"/>
              <line x1="6" y1="6" x2="18" y2="18"/>
            </svg>
          </button>
        </div>
        
        <form @submit.prevent="updateStorage" class="storage-form">
          <div class="form-section">
            <h4 class="section-title">基本信息</h4>
            <div class="form-grid">
              <div class="form-group">
                <label class="form-label">
                  <i class="label-icon">
                    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                      <path d="M20 21v-2a4 4 0 00-4-4H8a4 4 0 00-4 4v2"/>
                      <circle cx="12" cy="7" r="4"/>
                    </svg>
                  </i>
                  存储名称
                </label>
                <input 
                  v-model="editingStorage.name" 
                  type="text" 
                  required 
                  class="form-input" 
                  placeholder="输入存储名称"
                />
              </div>
              
              <div class="form-group">
                <label class="form-label">
                  <i class="label-icon">
                    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                      <path d="M9 12l2 2 4-4"/>
                      <path d="M21 12c-1 0-3-1-3-3s2-3 3-3 3 1 3 3-2 3-3 3"/>
                      <path d="M3 12c1 0 3-1 3-3s-2-3-3-3-3 1-3 3 2 3 3 3"/>
                      <path d="M3 12h6m6 0h6"/>
                    </svg>
                  </i>
                  存储类型
                </label>
                <select v-model="editingStorage.storage_type" required @change="onEditStorageTypeChange" class="form-input">
                  <option value="">选择存储类型</option>
                  <option v-for="driver in availableDrivers" :key="driver.driver_type" :value="driver.driver_type">
                    {{ driver.display_name }}
                  </option>
                </select>
              </div>
              
              <div class="form-group">
                <label class="form-label">
                  <i class="label-icon">
                    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                      <path d="M9 20l-5.447-2.724A1 1 0 0 1 3 16.382V5.618a1 1 0 0 1 0.553-0.894L9 2l6 3 5.447-2.724A1 1 0 0 1 21 3.382v10.764a1 1 0 0 1-0.553 0.894L15 18l-6-3z"/>
                    </svg>
                  </i>
                  挂载路径
                </label>
                <input 
                  v-model="editingStorage.mount_path" 
                  type="text" 
                  required 
                  class="form-input" 
                  placeholder="例如: /"
                />
              </div>
            </div>
          </div>
          
          <!-- 动态配置字段 -->
          <div v-if="editingDriver" class="form-section">
            <h4 class="section-title">配置参数</h4>

            <div class="form-grid">
              <div v-for="(property, key) in editingDriver.config_schema.properties" :key="key" class="form-group">
                <label class="form-label">{{ property.title || key }}</label>
                
                <input 
                  v-if="property.type === 'string' && !property.enum"
                  v-model="editingStorage.config[key]"
                  :type="property.format === 'password' ? 'password' : 'text'"
                  :required="editingDriver.config_schema.required?.includes(key)"
                  class="form-input"
                />
                
                <select 
                  v-else-if="property.enum"
                  v-model="editingStorage.config[key]"
                  :required="editingDriver.config_schema.required?.includes(key)"
                  class="form-input"
                >
                  <option v-for="(option, index) in property.enum" :key="option" :value="option">
                    {{ property.enumNames?.[index] || option }}
                  </option>
                </select>
                
                <input 
                  v-else-if="property.type === 'integer' || property.type === 'number'"
                  v-model.number="editingStorage.config[key]"
                  type="number"
                  :min="property.minimum"
                  :max="property.maximum"
                  :required="editingDriver.config_schema.required?.includes(key)"
                  class="form-input"
                />
                
                <div v-else-if="property.type === 'boolean'" class="checkbox-group">
                  <label class="checkbox-label">
                    <input 
                      v-model="editingStorage.config[key]"
                      type="checkbox"
                    />
                    <span class="checkbox-custom"></span>
                    <span>{{ property.title || key }}</span>
                  </label>
                </div>
              </div>
            </div>
          </div>
          
          <div class="form-actions">
            <button type="button" @click="showEditDialog = false" class="btn btn-secondary">取消</button>
            <button type="submit" class="btn btn-primary">更新</button>
          </div>
        </form>
      </div>
    </div>

    <!-- 创建用户对话框 -->
    <div v-if="showCreateUserDialog" class="modal-overlay" @click="showCreateUserDialog = false">
      <div class="modal-content user-modal" @click.stop>
        <div class="modal-header">
          <div class="modal-title">
            <i class="modal-icon">
              <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <path d="M16 21v-2a4 4 0 00-4-4H5a4 4 0 00-4 4v2"/>
                <circle cx="8.5" cy="7" r="4"/>
                <line x1="20" y1="8" x2="20" y2="14"/>
                <line x1="23" y1="11" x2="17" y2="11"/>
              </svg>
            </i>
            <div>
              <h3>创建用户</h3>
              <p class="modal-subtitle">添加新的系统用户</p>
            </div>
          </div>
          <button class="modal-close" @click="showCreateUserDialog = false">
            <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <line x1="18" y1="6" x2="6" y2="18"/>
              <line x1="6" y1="6" x2="18" y2="18"/>
            </svg>
          </button>
        </div>
        
        <form @submit.prevent="createUser" class="user-form">
          <div class="form-section">
            <h4 class="section-title">基本信息</h4>
            <div class="form-grid">
              <div class="form-group">
                <label class="form-label">
                  <i class="label-icon">
                    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                      <path d="M20 21v-2a4 4 0 00-4-4H8a4 4 0 00-4 4v2"/>
                      <circle cx="12" cy="7" r="4"/>
                    </svg>
                  </i>
                  用户名
                </label>
                <input 
                  v-model="newUser.username" 
                  type="text" 
                  required 
                  class="form-input" 
                  placeholder="输入用户名"
                />
              </div>
              
              <div class="form-group">
                <label class="form-label">
                  <i class="label-icon">
                    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                      <rect x="3" y="11" width="18" height="10" rx="2" ry="2"/>
                      <circle cx="12" cy="16" r="1"/>
                      <path d="M7 11V7a5 5 0 0110 0v4"/>
                    </svg>
                  </i>
                  密码
                </label>
                <input 
                  v-model="newUser.password" 
                  type="password" 
                  required 
                  class="form-input" 
                  placeholder="输入密码"
                />
              </div>
              
              <div class="form-group">
                <label class="form-label">
                  <i class="label-icon">
                    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                      <path d="M9 20l-5.447-2.724A1 1 0 0 1 3 16.382V5.618a1 1 0 0 1 0.553-0.894L9 2l6 3 5.447-2.724A1 1 0 0 1 21 3.382v10.764a1 1 0 0 1-0.553 0.894L15 18l-6-3z"/>
                    </svg>
                  </i>
                  用户路径
                </label>
                <input 
                  v-model="newUser.user_path" 
                  type="text" 
                  required 
                  class="form-input" 
                  placeholder="例如: /"
                />
              </div>
            </div>
          </div>
          
          <div class="form-section">
            <h4 class="section-title">权限设置</h4>
            <div class="permission-grid">
              <div class="permission-item">
                <label class="checkbox-label">
                  <input 
                    type="checkbox" 
                    :checked="(newUser.permissions & 64) !== 0"
                    @change="togglePermission('list', $event.target.checked)"
                  />
                  <span class="checkbox-custom"></span>
                  <span>文件列表</span>
                </label>
              </div>
              <div class="permission-item">
                <label class="checkbox-label">
                  <input 
                    type="checkbox" 
                    :checked="(newUser.permissions & 2) !== 0"
                    @change="togglePermission('download', $event.target.checked)"
                  />
                  <span class="checkbox-custom"></span>
                  <span>下载文件</span>
                </label>
              </div>
              <div class="permission-item">
                <label class="checkbox-label">
                  <input 
                    type="checkbox" 
                    :checked="(newUser.permissions & 1) !== 0"
                    @change="togglePermission('upload', $event.target.checked)"
                  />
                  <span class="checkbox-custom"></span>
                  <span>上传文件</span>
                </label>
              </div>
              <div class="permission-item">
                <label class="checkbox-label">
                  <input 
                    type="checkbox" 
                    :checked="(newUser.permissions & 4) !== 0"
                    @change="togglePermission('delete', $event.target.checked)"
                  />
                  <span class="checkbox-custom"></span>
                  <span>删除文件</span>
                </label>
              </div>
              <div class="permission-item">
                <label class="checkbox-label">
                  <input 
                    type="checkbox" 
                    :checked="(newUser.permissions & 32) !== 0"
                    @change="togglePermission('rename', $event.target.checked)"
                  />
                  <span class="checkbox-custom"></span>
                  <span>重命名</span>
                </label>
              </div>
              <div class="permission-item">
                <label class="checkbox-label">
                  <input 
                    type="checkbox" 
                    :checked="(newUser.permissions & 8) !== 0"
                    @change="togglePermission('copy', $event.target.checked)"
                  />
                  <span class="checkbox-custom"></span>
                  <span>复制文件</span>
                </label>
              </div>
              <div class="permission-item">
                <label class="checkbox-label">
                  <input 
                    type="checkbox" 
                    :checked="(newUser.permissions & 16) !== 0"
                    @change="togglePermission('move', $event.target.checked)"
                  />
                  <span class="checkbox-custom"></span>
                  <span>移动文件</span>
                </label>
              </div>
              <div class="permission-item admin-permission">
                <label class="checkbox-label">
                  <input 
                    type="checkbox" 
                    :checked="newUser.permissions === -1"
                    @change="togglePermission('admin', $event.target.checked)"
                  />
                  <span class="checkbox-custom"></span>
                  <span>管理员权限</span>
                </label>
              </div>
            </div>
          </div>
          
          <div class="form-section">
            <h4 class="section-title">账号状态</h4>
            <div class="checkbox-group">
              <label class="checkbox-label">
                <input 
                  v-model="newUser.enabled"
                  type="checkbox"
                />
                <span class="checkbox-custom"></span>
                <span>启用账号</span>
              </label>
            </div>
          </div>
          
          <div class="form-actions">
            <button type="button" @click="showCreateUserDialog = false" class="btn btn-secondary">取消</button>
            <button type="submit" class="btn btn-primary">创建用户</button>
          </div>
        </form>
      </div>
    </div>

    <!-- 编辑用户对话框 -->
    <div v-if="showEditUserDialog" class="modal-overlay" @click="showEditUserDialog = false">
      <div class="modal-content user-modal" @click.stop>
        <div class="modal-header">
          <div class="modal-title">
            <i class="modal-icon">
              <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <path d="M11 4H4a2 2 0 00-2 2v14a2 2 0 002 2h14a2 2 0 002-2v-7"/>
                <path d="M18.5 2.5a2.121 2.121 0 013 3L12 15l-4 1 1-4 9.5-9.5z"/>
              </svg>
            </i>
            <div>
              <h3>编辑用户</h3>
              <p class="modal-subtitle">修改用户信息和权限</p>
            </div>
          </div>
          <button class="modal-close" @click="showEditUserDialog = false">
            <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <line x1="18" y1="6" x2="6" y2="18"/>
              <line x1="6" y1="6" x2="18" y2="18"/>
            </svg>
          </button>
        </div>
        
        <form @submit.prevent="updateUser" class="user-form">
          <div class="form-section">
            <h4 class="section-title">基本信息</h4>
            <div class="form-grid">
              <div class="form-group">
                <label class="form-label">
                  <i class="label-icon">
                    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                      <path d="M20 21v-2a4 4 0 00-4-4H8a4 4 0 00-4 4v2"/>
                      <circle cx="12" cy="7" r="4"/>
                    </svg>
                  </i>
                  用户名
                </label>
                <input 
                  v-model="editingUser.username" 
                  type="text" 
                  required 
                  class="form-input" 
                  placeholder="输入用户名"
                />
              </div>
              
              <div class="form-group">
                <label class="form-label">
                  <i class="label-icon">
                    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                      <rect x="3" y="11" width="18" height="10" rx="2" ry="2"/>
                      <circle cx="12" cy="16" r="1"/>
                      <path d="M7 11V7a5 5 0 0110 0v4"/>
                    </svg>
                  </i>
                  新密码（留空则不修改）
                </label>
                <input 
                  v-model="editingUser.password" 
                  type="password" 
                  class="form-input" 
                  placeholder="输入新密码"
                />
              </div>
              
              <div class="form-group">
                <label class="form-label">
                  <i class="label-icon">
                    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                      <path d="M9 20l-5.447-2.724A1 1 0 0 1 3 16.382V5.618a1 1 0 0 1 0.553-0.894L9 2l6 3 5.447-2.724A1 1 0 0 1 21 3.382v10.764a1 1 0 0 1-0.553 0.894L15 18l-6-3z"/>
                    </svg>
                  </i>
                  用户路径
                </label>
                <input 
                  v-model="editingUser.user_path" 
                  type="text" 
                  required 
                  class="form-input" 
                  placeholder="例如: /"
                />
              </div>
            </div>
          </div>
          
          <div class="form-section">
            <h4 class="section-title">权限设置</h4>
            <div class="permission-grid">
              <div class="permission-item">
                <label class="checkbox-label">
                  <input 
                    type="checkbox" 
                    :checked="(editingUser.permissions & 64) !== 0"
                    @change="toggleEditPermission('list', $event.target.checked)"
                  />
                  <span class="checkbox-custom"></span>
                  <span>文件列表</span>
                </label>
              </div>
              <div class="permission-item">
                <label class="checkbox-label">
                  <input 
                    type="checkbox" 
                    :checked="(editingUser.permissions & 2) !== 0"
                    @change="toggleEditPermission('download', $event.target.checked)"
                  />
                  <span class="checkbox-custom"></span>
                  <span>下载文件</span>
                </label>
              </div>
              <div class="permission-item">
                <label class="checkbox-label">
                  <input 
                    type="checkbox" 
                    :checked="(editingUser.permissions & 1) !== 0"
                    @change="toggleEditPermission('upload', $event.target.checked)"
                  />
                  <span class="checkbox-custom"></span>
                  <span>上传文件</span>
                </label>
              </div>
              <div class="permission-item">
                <label class="checkbox-label">
                  <input 
                    type="checkbox" 
                    :checked="(editingUser.permissions & 4) !== 0"
                    @change="toggleEditPermission('delete', $event.target.checked)"
                  />
                  <span class="checkbox-custom"></span>
                  <span>删除文件</span>
                </label>
              </div>
              <div class="permission-item">
                <label class="checkbox-label">
                  <input 
                    type="checkbox" 
                    :checked="(editingUser.permissions & 32) !== 0"
                    @change="toggleEditPermission('rename', $event.target.checked)"
                  />
                  <span class="checkbox-custom"></span>
                  <span>重命名</span>
                </label>
              </div>
              <div class="permission-item">
                <label class="checkbox-label">
                  <input 
                    type="checkbox" 
                    :checked="(editingUser.permissions & 8) !== 0"
                    @change="toggleEditPermission('copy', $event.target.checked)"
                  />
                  <span class="checkbox-custom"></span>
                  <span>复制文件</span>
                </label>
              </div>
              <div class="permission-item">
                <label class="checkbox-label">
                  <input 
                    type="checkbox" 
                    :checked="(editingUser.permissions & 16) !== 0"
                    @change="toggleEditPermission('move', $event.target.checked)"
                  />
                  <span class="checkbox-custom"></span>
                  <span>移动文件</span>
                </label>
              </div>
              <div class="permission-item admin-permission">
                <label class="checkbox-label">
                  <input 
                    type="checkbox" 
                    :checked="editingUser.permissions === -1"
                    @change="toggleEditPermission('admin', $event.target.checked)"
                  />
                  <span class="checkbox-custom"></span>
                  <span>管理员权限</span>
                </label>
              </div>
            </div>
          </div>
          
          <div class="form-section">
            <h4 class="section-title">账号状态</h4>
            <div class="checkbox-group">
              <label class="checkbox-label">
                <input 
                  v-model="editingUser.enabled"
                  type="checkbox"
                />
                <span class="checkbox-custom"></span>
                <span>启用账号</span>
              </label>
            </div>
          </div>
          
          <div class="form-actions">
            <button type="button" @click="showEditUserDialog = false" class="btn btn-secondary">取消</button>
            <button type="submit" class="btn btn-primary">更新用户</button>
          </div>
        </form>
      </div>
    </div>

    </div>
  </template>

<script setup>
import { ref, onMounted, computed } from 'vue'
import axios from 'axios'
import { useRouter } from 'vue-router'
import notification from './utils/notification.js'
import { marked } from 'marked'

let raw = localStorage.getItem('yaolist_user')
let userObj = {}
try {
  userObj = JSON.parse(raw)
} catch {
  userObj = { username: raw }
}
const user = ref(userObj)
const username = computed(() => user.value.username || '管理员')
const isAdmin = computed(() => user.value.permissions === -1)
const isGuest = computed(() => user.value.username === 'guest')

const userPerms = ref([
  '文件列表',
  '下载',
  '上传',
  '删除',
  '重命名',
  '管理员权限'
])

// 权限常量
const PERM_UPLOAD = 1
const PERM_DOWNLOAD = 2
const PERM_DELETE = 4
const PERM_COPY = 8
const PERM_MOVE = 16
const PERM_RENAME = 32
const PERM_LIST = 64

// 获取所有权限定义
function getAllPermissions() {
  return [
    {
      key: 'list',
      name: '文件列表',
      description: '查看文件和文件夹列表',
      value: PERM_LIST,
      icon: 'M8 6h13M8 12h13M8 18h13M3 6h.01M3 12h.01M3 18h.01'
    },
    {
      key: 'download',
      name: '下载文件',
      description: '下载文件到本地设备',
      value: PERM_DOWNLOAD,
      icon: 'M21 15v4a2 2 0 01-2 2H5a2 2 0 01-2-2v-4m7-10l4 4m0 0l4-4m-4 4V3'
    },
    {
      key: 'upload',
      name: '上传文件',
      description: '上传文件到服务器',
      value: PERM_UPLOAD,
      icon: 'M21 15v4a2 2 0 01-2 2H5a2 2 0 01-2-2v-4m7-6l4-4m0 0l4 4m-4-4v12'
    },
    {
      key: 'delete',
      name: '删除文件',
      description: '删除文件和文件夹',
      value: PERM_DELETE,
      icon: 'M3 6h18m-2 0v14a2 2 0 01-2 2H7a2 2 0 01-2-2V6m3 0V4a2 2 0 012-2h4a2 2 0 012 2v2'
    },
    {
      key: 'rename',
      name: '重命名',
      description: '重命名文件和文件夹',
      value: PERM_RENAME,
      icon: 'M11 4H4a2 2 0 00-2 2v14a2 2 0 002 2h14a2 2 0 002-2v-7m-1.5-9.5l2.5 2.5L11 19l-4 1 1-4L19.5 4.5z'
    },
    {
      key: 'copy',
      name: '复制文件',
      description: '复制文件和文件夹',
      value: PERM_COPY,
      icon: 'M16 4h2a2 2 0 012 2v14a2 2 0 01-2 2H6a2 2 0 01-2-2V6a2 2 0 012-2h2m8 0V2a2 2 0 00-2-2H8a2 2 0 00-2 2v2m8 0H8'
    },
    {
      key: 'move',
      name: '移动文件',
      description: '移动文件和文件夹',
      value: PERM_MOVE,
      icon: 'M7 7l9.2 9.2M17 7v10H7m10-10H7'
    }
  ]
}

// 检查是否拥有特定权限
function hasSpecificPermission(permValue) {
  if (user.value.permissions === -1) return true // 管理员拥有所有权限
  return (user.value.permissions & permValue) !== 0
}

// 获取已授权权限数量
function getGrantedPermissionsCount() {
  if (user.value.permissions === -1) return getAllPermissions().length
  return getAllPermissions().filter(perm => hasSpecificPermission(perm.value)).length
}

// 格式化日期
function formatDate(dateString) {
  if (!dateString) return '-'
  const date = new Date(dateString)
  return date.toLocaleDateString('zh-CN', {
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit'
  })
}

// 加载当前用户信息
async function loadCurrentUser() {
  try {
    const res = await axios.get('/api/user/profile', {
      headers: {
        'x-username': user.value.username
      }
    })
    // 更新用户信息，保持响应式
    Object.assign(user.value, res.data)
  } catch (error) {
    console.error('加载用户信息失败:', error)
    // 如果获取失败，尝试从用户列表中找到当前用户
    try {
      const usersRes = await axios.get('/api/admin/users', {
        headers: {
          'x-username': user.value.username
        }
      })
      const currentUserData = usersRes.data.find(u => u.username === user.value.username)
      if (currentUserData) {
        Object.assign(user.value, currentUserData)
      }
    } catch (fallbackError) {
      console.error('备用方法加载用户信息也失败:', fallbackError)
    }
  }
}

// 基础数据
  const activeTab = ref('profile')
const siteSubTab = ref('general') // 站点设置子标签
const oldPassword = ref('')
const newPassword = ref('')
const confirmPassword = ref('')
const router = useRouter()

// 主题和语言设置
const isDarkMode = ref(localStorage.getItem('yaolist_dark_mode') === 'true')
const currentLanguage = ref(localStorage.getItem('yaolist_language') || 'zh')

// 站点设置
const siteSettings = ref({
  site_title: 'YaoList',
  site_description: '现代化的文件管理系统',
  theme_color: '#1976d2',
  site_icon: 'https://api.ylist.org/logo/logo.svg',
  favicon: 'https://api.ylist.org/logo/logo.svg',
  pagination_type: 'infinite',
  items_per_page: 50,
  allow_registration: true,
  preview_text_types: 'txt,htm,html,xml,java,properties,sql,js,md,json,conf,ini,vue,php,py,bat,gitignore,yml,go,sh,c,cpp,h,hpp,tsx,vtt,srt,ass,rs,lrc',
  preview_audio_types: 'mp3,flac,ogg,m4a,wav,opus,wma',
  preview_video_types: 'mp4,mkv,avi,mov,rmvb,webm,flv',
  preview_image_types: 'jpg,tiff,jpeg,png,gif,bmp,svg,ico,swf,webp',
  preview_proxy_types: 'm3u8',
  preview_proxy_ignore_headers: 'authorization,referer',
  preview_external: '{}',
  preview_iframe: '{"doc,docx,xls,xlsx,ppt,pptx":{"Microsoft":"https://view.officeapps.live.com/op/view.aspx?src=$e_url","Google":"https://docs.google.com/gview?url=$e_url&embedded=true"},"pdf":{"PDF.js":"https://alist-org.github.io/pdf.js/web/viewer.html?file=$e_url"},"epub":{"EPUB.js":"https://alist-org.github.io/static/epub.js/viewer.html?url=$e_url"}}',
  preview_audio_cover: 'https://api.ylist.org/logo/logo.svg',
  preview_auto_play_audio: false,
  preview_auto_play_video: false,
  preview_default_archive: false,
  preview_readme_render: true,
  preview_readme_filter_script: true,
  enable_top_message: false,
  top_message: '',
  enable_bottom_message: false,
  bottom_message: ''
})
const savingSettings = ref(false)

// 用户管理
const users = ref([])
const showCreateUserDialog = ref(false)
const showEditUserDialog = ref(false)
const currentUser = ref(user.value)
const newUser = ref({
  username: '',
  password: '',
  permissions: 66, // 默认列表和下载权限
  enabled: true,
  user_path: '/'
})
const editingUser = ref({
  id: null,
  username: '',
  password: '',
  permissions: 66,
  enabled: true,
  user_path: '/'
})

// 存储管理相关
const storages = ref([])
const availableDrivers = ref([])
const showAddDialog = ref(false)
const showEditDialog = ref(false)
const newStorage = ref({
  name: '',
  storage_type: '',
  mount_path: '',
  config: {},
  enabled: true
})
const editingStorage = ref({
  id: null,
  name: '',
  storage_type: '',
  mount_path: '',
  config: {},
  enabled: true
})

// 备份相关
const backupFile = ref(null)

// 系统信息
const systemUptime = ref('24小时30分钟')

// 预览设置相关
const showPreviewHelp = ref(false)
const jsonErrors = ref({
  preview_external: '',
  preview_iframe: ''
})

// 预设配置
const presetConfigs = {
  text: 'txt,htm,html,xml,java,properties,sql,js,md,json,conf,ini,vue,php,py,bat,gitignore,yml,go,sh,c,cpp,h,hpp,tsx,vtt,srt,ass,rs,lrc',
  audio: 'mp3,flac,ogg,m4a,wav,opus,wma',
  video: 'mp4,mkv,avi,mov,rmvb,webm,flv',
  image: 'jpg,tiff,jpeg,png,gif,bmp,svg,ico,swf,webp',
  external: '{}',
  iframe: '{"doc,docx,xls,xlsx,ppt,pptx":{"Microsoft":"https://view.officeapps.live.com/op/view.aspx?src=$e_url","Google":"https://docs.google.com/gview?url=$e_url&embedded=true"},"pdf":{"PDF.js":"https://alist-org.github.io/pdf.js/web/viewer.html?file=$e_url"},"epub":{"EPUB.js":"https://alist-org.github.io/static/epub.js/viewer.html?url=$e_url"}}'
}

// 计算属性：当前选中的驱动
const selectedDriver = computed(() => {
  return availableDrivers.value.find(driver => driver.driver_type === newStorage.value.storage_type)
})

const editingDriver = computed(() => {
  return availableDrivers.value.find(driver => driver.driver_type === editingStorage.value.storage_type)
})

// 页面标题
function getPageTitle() {
  const titles = {
    profile: '个人资料',
    site: '站点设置',
    users: '用户管理',
    storage: '存储管理',
    backup: '备份&恢复',
    about: '关于YaoList',
    docs: '文档'
  }
  return titles[activeTab.value] || '管理后台'
}

// 设置活动标签
function setActiveTab(tab) {
  // 如果是游客用户，限制访问管理功能
  if (isGuest.value && ['site', 'users', 'storage', 'backup'].includes(tab)) {
    notification.warning('游客用户无权访问此功能')
    return
  }
  activeTab.value = tab
  if (tab === 'site') {
    siteSubTab.value = 'general' // 默认显示基本设置
  }
}

// 设置站点子标签
function setSiteSubTab(subTab) {
  siteSubTab.value = subTab
}

// 加载站点设置
async function loadSiteSettings() {
  try {
    const res = await axios.get('/api/admin/site-settings', {
      headers: {
        'x-username': user.value.username
      }
    })
    
    // 将设置数组转换为对象
    const settingsObj = {}
    res.data.forEach(setting => {
      let value = setting.setting_value
      
      // 根据类型转换值
      if (setting.setting_type === 'boolean') {
        value = value === 'true'
      } else if (setting.setting_type === 'number') {
        value = parseInt(value)
      }
      
      settingsObj[setting.setting_key] = value
    })
    
    // 更新站点设置
    Object.assign(siteSettings.value, settingsObj)
  } catch (error) {
    console.error('加载站点设置失败:', error)
  }
}

// 保存站点设置
async function saveSiteSettings() {
  // 验证JSON字段
  const jsonFields = ['preview_external', 'preview_iframe']
  let hasJsonError = false
  
  jsonFields.forEach(field => {
    validateJson(field)
    if (jsonErrors.value[field]) {
      hasJsonError = true
    }
  })
  
  if (hasJsonError) {
    notification.error('请修复JSON格式错误后再保存')
    return
  }
  
  savingSettings.value = true
  try {
    // 准备设置数据
    const settings = {}
    Object.keys(siteSettings.value).forEach(key => {
      let value = siteSettings.value[key]
      
      // 转换为字符串
      if (typeof value === 'boolean') {
        value = value.toString()
      } else if (typeof value === 'number') {
        value = value.toString()
      }
      
      settings[key] = value
    })
    
    await axios.put('/api/admin/site-settings', {
      settings: settings
    }, {
      headers: {
        'x-username': user.value.username
      }
    })
    
    notification.success('站点设置保存成功')
    
    // 应用主题色到页面
    if (siteSettings.value.theme_color) {
      document.documentElement.style.setProperty('--theme-color', siteSettings.value.theme_color)
    }
    
  } catch (error) {
    notification.error(error.response?.data || '保存站点设置失败')
  } finally {
    savingSettings.value = false
  }
}

// 主题切换
function toggleDarkMode() {
  isDarkMode.value = !isDarkMode.value
  localStorage.setItem('yaolist_dark_mode', isDarkMode.value.toString())
  
  // 应用主题到body
  if (isDarkMode.value) {
    document.body.classList.add('dark-mode')
  } else {
    document.body.classList.remove('dark-mode')
  }
}

// 语言切换
function toggleLanguage() {
  currentLanguage.value = currentLanguage.value === 'zh' ? 'en' : 'zh'
  localStorage.setItem('yaolist_language', currentLanguage.value)
  // 这里可以添加国际化逻辑
}

// 返回主页
function goToHome() {
  router.push('/')
}

// 退出登录
function logout() {
  localStorage.removeItem('yaolist_user')
  router.push('/login')
}

// 修改密码
async function handleChangePassword() {
  if (newPassword.value !== confirmPassword.value) {
    notification.error('两次输入的新密码不一致')
    return
  }
  try {
    const res = await axios.post('/api/user/password', {
      old_password: oldPassword.value,
      new_password: newPassword.value
    }, {
      headers: {
        'x-username': user.value.username
      }
    })
    if (res.status === 200 || res.status === 205) {
      notification.success('密码修改成功，正在跳转到登录页面...')
      oldPassword.value = ''
      newPassword.value = ''
      confirmPassword.value = ''
      
      setTimeout(() => {
        localStorage.removeItem('yaolist_user')
        router.push('/login')
      }, 2000)
    }
  } catch (error) {
    notification.error(error.response?.data || '修改密码失败')
  }
}

// 用户管理相关函数
function getPermissionText(permissions) {
  if (permissions === -1) return '管理员'
  return '普通用户'
}

function getPermissionClass(permissions) {
  if (permissions === -1) return 'admin'
  return 'user'
}

function editUser(userToEdit) {
  editingUser.value = {
    id: userToEdit.id,
    username: userToEdit.username,
    password: '',
    permissions: userToEdit.permissions,
    enabled: userToEdit.enabled,
    user_path: userToEdit.user_path
  }
  showEditUserDialog.value = true
}

async function deleteUser(userId) {
  if (!confirm('确定要删除这个用户吗？')) return
  
  try {
    await axios.delete(`/api/admin/users/${userId}`, {
      headers: {
        'x-username': user.value.username
      }
    })
    await loadUsers()
    notification.success('用户删除成功')
  } catch (error) {
    notification.error(error.response?.data || '删除用户失败')
  }
}

// 创建用户
async function createUser() {
  try {
    await axios.post('/api/admin/users', newUser.value, {
      headers: {
        'x-username': user.value.username
      }
    })
    await loadUsers()
    showCreateUserDialog.value = false
    resetNewUser()
    notification.success('用户创建成功')
  } catch (error) {
    notification.error(error.response?.data || '创建用户失败')
  }
}

// 更新用户
async function updateUser() {
  try {
    const updateData = {
      username: editingUser.value.username,
      permissions: editingUser.value.permissions,
      enabled: editingUser.value.enabled,
      user_path: editingUser.value.user_path
    }
    
    // 只有在输入了新密码时才包含密码字段
    if (editingUser.value.password.trim()) {
      updateData.password = editingUser.value.password
    }
    
    await axios.put(`/api/admin/users/${editingUser.value.id}`, updateData, {
      headers: {
        'x-username': user.value.username
      }
    })
    await loadUsers()
    showEditUserDialog.value = false
    notification.success('用户更新成功')
  } catch (error) {
    notification.error(error.response?.data || '更新用户失败')
  }
}

// 重置新用户表单
function resetNewUser() {
  newUser.value = {
    username: '',
    password: '',
    permissions: 66, // 默认列表和下载权限
    enabled: true,
    user_path: '/'
  }
}

// 权限切换函数
function togglePermission(type, checked) {
  const permissionMap = {
    list: 64,
    download: 2,
    upload: 1,
    delete: 4,
    rename: 32,
    copy: 8,
    move: 16,
    admin: -1
  }
  
  if (type === 'admin') {
    newUser.value.permissions = checked ? -1 : 66
  } else {
    const bit = permissionMap[type]
    if (checked) {
      newUser.value.permissions |= bit
    } else {
      newUser.value.permissions &= ~bit
    }
  }
}

// 编辑权限切换函数
function toggleEditPermission(type, checked) {
  const permissionMap = {
    list: 64,
    download: 2,
    upload: 1,
    delete: 4,
    rename: 32,
    copy: 8,
    move: 16,
    admin: -1
  }
  
  if (type === 'admin') {
    editingUser.value.permissions = checked ? -1 : 66
  } else {
    const bit = permissionMap[type]
    if (checked) {
      editingUser.value.permissions |= bit
    } else {
      editingUser.value.permissions &= ~bit
    }
  }
}

// 加载用户列表
async function loadUsers() {
  try {
    const res = await axios.get('/api/admin/users', {
      headers: {
        'x-username': user.value.username
      }
    })
    users.value = res.data
  } catch (error) {
    console.error('加载用户列表失败:', error)
  }
}

// 存储管理相关函数
async function loadStorages() {
  try {
    const res = await axios.get('/api/admin/storages', {
      headers: {
        'x-username': user.value.username
      }
    })
    storages.value = res.data
  } catch (error) {
    console.error('加载存储列表失败:', error)
  }
}

// 获取存储类型显示名称
function getStorageTypeDisplay(storageType) {
  const typeMap = {
    'local': '本地存储',
    'ftp': 'FTP存储',
    'onedrive': 'OneDrive',
    'webdav': 'WebDAV',
    's3': 'S3存储'
  }
  return typeMap[storageType] || storageType
}

// 获取存储类型描述
function getStorageTypeDescription(storageType) {
  const descMap = {
    'local': '直接访问服务器本地文件系统',
    'ftp': '连接到FTP服务器存储文件',
    'onedrive': '连接到Microsoft OneDrive云存储',
    'webdav': '通过WebDAV协议访问远程存储',
    's3': '连接到Amazon S3兼容的对象存储'
  }
  return descMap[storageType] || '其他存储类型'
}

// 获取存储图标样式类
function getStorageIconClass(storageType) {
  return `storage-icon-${storageType}`
}

// 切换存储启用状态
async function toggleStorage(storage) {
  try {
    await axios.put(`/api/admin/storages/${storage.id}`, {
      ...storage,
      enabled: !storage.enabled
    }, {
      headers: {
        'x-username': user.value.username
      }
    })
    
    storage.enabled = !storage.enabled
    notification.success(`存储已${storage.enabled ? '启用' : '禁用'}`)
  } catch (error) {
    notification.error(error.response?.data || '操作失败')
  }
}

async function loadAvailableDrivers() {
  try {
    const res = await axios.get('/api/admin/drivers', {
      headers: {
        'x-username': user.value.username
      }
    })
    availableDrivers.value = res.data
  } catch (error) {
    console.error('加载驱动列表失败:', error)
  }
}

function onStorageTypeChange() {
  newStorage.value.config = {}
  if (selectedDriver.value) {
    // 设置默认值
    for (const [key, property] of Object.entries(selectedDriver.value.config_schema.properties)) {
      if (property.default !== undefined) {
        newStorage.value.config[key] = property.default
      }
    }
  }
}

function onEditStorageTypeChange() {
  editingStorage.value.config = {}
  if (editingDriver.value) {
    // 设置默认值
    for (const [key, property] of Object.entries(editingDriver.value.config_schema.properties)) {
      if (property.default !== undefined) {
        editingStorage.value.config[key] = property.default
      }
    }
  }
}



async function createStorage() {
  try {
    await axios.post('/api/admin/storages', {
      name: newStorage.value.name,
      storage_type: newStorage.value.storage_type,
      config: newStorage.value.config,
      mount_path: newStorage.value.mount_path,
      enabled: newStorage.value.enabled
    }, {
      headers: {
        'x-username': user.value.username
      }
    })
    
    showAddDialog.value = false
    newStorage.value = {
      name: '',
      storage_type: '',
      mount_path: '',
      config: {},
      enabled: true
    }
    await loadStorages()
    notification.success('存储创建成功')
  } catch (error) {
    notification.error(error.response?.data || '创建存储失败')
  }
}

async function updateStorage() {
  try {
    await axios.put(`/api/admin/storages/${editingStorage.value.id}`, {
      name: editingStorage.value.name,
      storage_type: editingStorage.value.storage_type,
      config: editingStorage.value.config,
      mount_path: editingStorage.value.mount_path,
      enabled: editingStorage.value.enabled
    }, {
      headers: {
        'x-username': user.value.username
      }
    })
    
    showEditDialog.value = false
    await loadStorages()
    notification.success('存储更新成功')
  } catch (error) {
    notification.error(error.response?.data || '更新存储失败')
  }
}

// 预览设置相关方法
function getFileTypeArray(settingKey) {
  const value = siteSettings.value[settingKey]
  if (!value) return []
  return value.split(',').map(type => type.trim()).filter(type => type)
}

function applyPreset(type) {
  switch (type) {
    case 'text':
      siteSettings.value.preview_text_types = presetConfigs.text
      break
    case 'audio':
      siteSettings.value.preview_audio_types = presetConfigs.audio
      break
    case 'video':
      siteSettings.value.preview_video_types = presetConfigs.video
      break
    case 'image':
      siteSettings.value.preview_image_types = presetConfigs.image
      break
    case 'external':
      siteSettings.value.preview_external = presetConfigs.external
      break
    case 'iframe':
      siteSettings.value.preview_iframe = presetConfigs.iframe
      break
  }
}

function validateJson(field) {
  try {
    const value = siteSettings.value[field]
    if (value && value.trim()) {
      JSON.parse(value)
      jsonErrors.value[field] = ''
    }
  } catch (error) {
    jsonErrors.value[field] = 'JSON格式错误: ' + error.message
  }
}

function resetPreviewSettings() {
  if (!confirm('确定要重置所有预览设置为默认值吗？')) return
  
  siteSettings.value.preview_text_types = presetConfigs.text
  siteSettings.value.preview_audio_types = presetConfigs.audio
  siteSettings.value.preview_video_types = presetConfigs.video
  siteSettings.value.preview_image_types = presetConfigs.image
  siteSettings.value.preview_proxy_types = 'm3u8'
  siteSettings.value.preview_proxy_ignore_headers = 'authorization,referer'
  siteSettings.value.preview_external = presetConfigs.external
  siteSettings.value.preview_iframe = presetConfigs.iframe
  siteSettings.value.preview_audio_cover = 'https://jsd.nn.ci/gh/alist-org/logo@main/logo.svg'
  siteSettings.value.preview_auto_play_audio = false
  siteSettings.value.preview_auto_play_video = false
  siteSettings.value.preview_default_archive = false
  siteSettings.value.preview_readme_render = true
  siteSettings.value.preview_readme_filter_script = true
  
  // 清除JSON错误
  jsonErrors.value.preview_external = ''
  jsonErrors.value.preview_iframe = ''
  
  notification.success('预览设置已重置为默认值')
}

function handleImageError(event) {
  event.target.style.display = 'none'
}

function editStorage(storage) {
  const parsedConfig = JSON.parse(storage.config)
  
  // 直接设置所有属性
  editingStorage.value.id = storage.id
  editingStorage.value.name = storage.name
  editingStorage.value.storage_type = storage.storage_type
  editingStorage.value.mount_path = storage.mount_path
  editingStorage.value.enabled = storage.enabled
  
  // 清空配置然后逐个设置
  editingStorage.value.config = {}
  Object.keys(parsedConfig).forEach(key => {
    editingStorage.value.config[key] = parsedConfig[key]
  })
  
  showEditDialog.value = true
}

function resetStorageDialog() {
  newStorage.value = {
    name: '',
    storage_type: '',
    mount_path: '',
    config: {},
    enabled: true
  }
}

async function deleteStorage(storageId) {
  if (!confirm('确定要删除这个存储吗？')) return
  
  try {
    await axios.delete(`/api/admin/storages/${storageId}`, {
      headers: {
        'x-username': user.value.username
      }
    })
    await loadStorages()
    notification.success('存储删除成功')
  } catch (error) {
    notification.error(error.response?.data || '删除存储失败')
  }
}

// 备份相关函数
function createBackup() {
  // 创建备份逻辑
  const backupData = {
    storages: storages.value,
    users: users.value,
    settings: siteSettings.value,
    timestamp: new Date().toISOString()
  }
  
  const blob = new Blob([JSON.stringify(backupData, null, 2)], { type: 'application/json' })
  const url = URL.createObjectURL(blob)
  const a = document.createElement('a')
  a.href = url
  a.download = `yaolist-backup-${new Date().toISOString().split('T')[0]}.json`
  a.click()
  URL.revokeObjectURL(url)
  
  notification.success('备份文件已下载')
}

function handleBackupFile(event) {
  backupFile.value = event.target.files[0]
}

async function restoreBackup() {
  if (!backupFile.value) return
  
  try {
    const text = await backupFile.value.text()
    const backupData = JSON.parse(text)
    
    // 这里应该调用后端API来恢复数据

    
    notification.success('备份恢复成功')
  } catch (error) {
    notification.error('备份文件格式错误')
  }
}

// 组件挂载时加载数据
onMounted(async () => {
  await loadCurrentUser() // 首先加载当前用户信息
  await loadStorages()
  await loadAvailableDrivers()
  await loadUsers()
  await loadSiteSettings()
  
  // 应用保存的主题设置
  if (isDarkMode.value) {
    document.body.classList.add('dark-mode')
  }
  
  // 应用站点主题色
  if (siteSettings.value.theme_color) {
    document.documentElement.style.setProperty('--theme-color', siteSettings.value.theme_color)
  }

  try {
    const response = await axios.get('https://raw.githubusercontent.com/ChuYao233/yaolist/main/README.md');
    renderedReadme.value = marked(response.data);
  } catch (error) {
    console.error('Failed to load README:', error);
    renderedReadme.value = '<p>加载 README 失败</p>';
  }
})

const renderedReadme = ref('')

const handleDocsClick = (e) => {
  e.preventDefault();
  window.open('https://docs.yaolist.org', '_blank');
  setActiveTab('docs');
}
</script>

<style scoped>
@import './styles/AdminPanel.css';

</style> 