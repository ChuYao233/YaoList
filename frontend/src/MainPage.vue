<template>
  <!-- 如果是文件预览，显示FileDetail组件 -->
  <FileDetail v-if="isFilePreview" />
  <!-- 否则显示文件列表 -->
  <div v-else class="yaolist-flex-root" :class="{ 'dark-mode': isDarkMode }" :style="backgroundStyle">
    <!-- 主题切换按钮 -->
    <div class="theme-toggle-container">
      <button class="theme-toggle-btn" @click="toggleDarkMode" :title="isDarkMode ? '切换到浅色模式' : '切换到深色模式'">
        <svg v-if="isDarkMode" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
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
        <svg v-else width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <path d="M21 12.79A9 9 0 1111.21 3 7 7 0 0021 12.79z"/>
        </svg>
      </button>
    </div>
    
    <!-- 中间内容区域 -->
    <div class="yaolist-flex-center">
      <!-- 站点标题 -->
      <div class="yaolist-title title-above-card">
        <div class="title-left">
          <img v-if="siteInfo.site_icon" class="yaolist-logo-large" :src="siteInfo.site_icon" alt="logo" @error="onLogoError" />
          <img v-else class="yaolist-logo-large" src="/favicon.ico" alt="logo" @error="onLogoError" />
          <span class="title-text">{{ siteInfo.site_title }}</span>
        </div>
      </div>

      <!-- 顶部自定义信息 -->
      <div v-if="siteInfo.enable_top_message && siteInfo.top_message" class="custom-message-card top-message glass-effect">
      <div class="markdown-content" v-html="renderContent(siteInfo.top_message)"></div>
    </div>

      <div 
        class="yaolist-card"
        :class="{ 
          'drag-over': dragOver,
          'glass-effect': siteInfo.enable_glass_effect 
        }"
        @dragover.prevent="handleDragOver"
        @dragleave.prevent="handleDragLeave"
        @drop.prevent="handleDrop"
      >
        <!-- 路径面包屑 -->
        <div class="yaolist-path-breadcrumb">
          <template v-for="(crumb, idx) in pathBreadcrumbs" :key="crumb.path">
            <span
              class="yaolist-breadcrumb clickable"
              @click="navigateTo(crumb.path, true)"
            >
              {{ crumb.name }}
            </span>
            <span v-if="idx !== pathBreadcrumbs.length - 1" class="yaolist-breadcrumb-sep">/</span>
          </template>
        </div>
        <!-- 文件表格 -->
        <el-table
          :data="paginatedFiles"
          class="yaolist-table"
          :header-cell-style="{ background: '#f8f9fa', boxShadow: 'none', borderBottom: 'none', color: '#374151', fontWeight: '600' }"
          :cell-style="{ border: 'none', padding: '8px 16px', background: 'transparent' }"
          v-loading="loading"
          @row-click="handleRowClick"
          @row-contextmenu="handleRowContextMenu"
          @row-mouseenter="onRowMouseEnter"
          @row-mouseleave="onRowMouseLeave"
          :row-class-name="getRowClassName"
          size="small"
          ref="fileTable"
          @selection-change="handleSelectionChange"
        >
          <!-- 复选框列 -->
          <el-table-column 
            v-if="checkboxMode" 
            type="selection" 
            width="55"
            :selectable="row => !row.is_dir || hasPermission(PERM_DELETE)"
          />
          <el-table-column prop="name" label="名称" min-width="200">
            <template #default="{ row }">
              <div class="file-name-container">
                <span class="file-name clickable" @click="handleFileClick(row)">
                  <span 
                    class="file-icon" 
                    :style="{ color: getFileIcon(row.name, row.is_dir).color }"
                    v-html="getFileIcon(row.name, row.is_dir).svg"
                  ></span>
                  <span class="file-name-text">{{ row.name }}</span>
                </span>
              </div>
            </template>
          </el-table-column>
                      <el-table-column prop="size" label="大小" width="120" align="right">
              <template #default="{ row }">
                <span v-if="!row.is_dir" class="file-size" :title="`${row.size.toLocaleString()} 字节`">{{ formatFileSize(row.size) }}</span>
                <span v-else class="file-size">-</span>
              </template>
            </el-table-column>
          <el-table-column prop="modified" label="修改时间" width="180">
            <template #default="{ row }">
              <span class="file-date">{{ formatDate(row.modified) }}</span>
            </template>
          </el-table-column>
          <el-table-column width="120" align="right" label="操作">
            <template #default="{ row, $index }">
              <template v-if="!row.is_dir">
                <div class="file-action-group" :class="{ show: hoverRowIndex === $index }">
                  <el-tooltip content="下载文件" placement="top">
                    <button class="file-action-btn download" @click.stop="downloadFile(row)">
                      <svg width="16" height="16" viewBox="0 0 24 24">
                        <path fill="#1976d2" d="M5 20h14v-2H5v2zm7-18c-.55 0-1 .45-1 1v8.59l-3.29-3.3a.996.996 0 1 0-1.41 1.41l5 5c.39.39 1.02.39 1.41 0l5-5a.996.996 0 1 0-1.41-1.41L13 11.59V3c0-.55-.45-1-1-1z"/>
                      </svg>
                    </button>
                  </el-tooltip>
                  <el-tooltip content="复制链接" placement="top">
                    <button class="file-action-btn copy" @click.stop="copyLink(row)">
                      <svg width="16" height="16" viewBox="0 0 24 24">
                        <path fill="#7c4dff" d="M16 1H4c-1.1 0-2 .9-2 2v14h2V3h12V1zm3 4H8c-1.1 0-2 .9-2 2v14c0 1.1.9 2 2 2h11c1.1 0 2-.9 2-2V7c0-1.1-.9-2-2-2zm0 16H8V7h11v14z"/>
                      </svg>
                    </button>
                  </el-tooltip>
                </div>
              </template>
            </template>
          </el-table-column>
        </el-table>
        <div class="yaolist-pagination" v-if="files.length > 0">
          <div class="yaolist-pagination-inner">
            <el-pagination
              v-model:current-page="currentPage"
              :page-size="pageSize"
              :total="files.length"
              layout="prev, pager, next"
              @current-change="handlePageChange"
            />
          </div>
        </div>
        
        <!-- 批量操作栏 -->
        <div v-if="checkboxMode && selectedFiles.length > 0" class="batch-actions">
          <div class="batch-info">
            已选择 {{ selectedFiles.length }} 个项目
          </div>
          <div class="batch-buttons">
            <button 
              v-if="hasPermission(PERM_DELETE)" 
              class="batch-btn delete" 
              @click="batchDelete"
            >
              <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <polyline points="3,6 5,6 21,6"/>
                <path d="M19 6v14a2 2 0 01-2 2H7a2 2 0 01-2-2V6m3 0V4a2 2 0 012-2h4a2 2 0 012 2v2"/>
                <line x1="10" y1="11" x2="10" y2="17"/>
                <line x1="14" y1="11" x2="14" y2="17"/>
              </svg>
              删除
            </button>
            <button class="batch-btn cancel" @click="clearSelection">
              取消选择
            </button>
          </div>
        </div>

      </div>

      <!-- 底部自定义信息 -->
      <div v-if="siteInfo.enable_bottom_message && siteInfo.bottom_message" class="custom-message-card bottom-message glass-effect">
      <div class="markdown-content" v-html="renderContent(siteInfo.bottom_message)"></div>
      </div>
    </div>
    
    <!-- 悬浮操作菜单 -->
    <div class="floating-menu" v-if="!isFilePreview">
      <div class="floating-menu-card">
        <button 
          v-if="hasPermission(PERM_UPLOAD)" 
          class="floating-menu-item upload" 
          @click="openUploadDialog"
          title="上传文件"
        >
          <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <path d="M21 15v4a2 2 0 01-2 2H5a2 2 0 01-2-2v-4"/>
            <polyline points="7,10 12,5 17,10"/>
            <line x1="12" y1="5" x2="12" y2="15"/>
          </svg>
        </button>
        
        <button 
          class="floating-menu-item create-folder" 
          @click="openCreateFolderDialog"
          title="创建文件夹"
        >
          <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/>
            <line x1="12" y1="11" x2="12" y2="17"/>
            <line x1="9" y1="14" x2="15" y2="14"/>
          </svg>
        </button>
        
        <button 
          class="floating-menu-item refresh" 
          @click="refreshFiles"
          title="刷新"
        >
          <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <path d="M23 4v6h-6"/>
            <path d="M20.49 15a9 9 0 1 1-2.12-9.36L23 10"/>
          </svg>
        </button>
        
        <button 
          class="floating-menu-item checkbox-toggle" 
          @click="toggleCheckboxMode"
          :title="checkboxMode ? '取消选择' : '批量选择'"
        >
          <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <polyline points="9,11 12,14 22,4"/>
            <path d="M21 12v7a2 2 0 01-2 2H5a2 2 0 01-2-2V5a2 2 0 01-2-2h11"/>
          </svg>
        </button>
      </div>
    </div>
    
    <!-- 底部登录信息 -->
    <div class="yaolist-bottom-userinfo userinfo-float">
      <template v-if="user && user.username && user.username !== 'guest'">
        <span style="font-weight: bold; color: #333; cursor:pointer;" @click="router.push('/admin')">{{ user.username }}</span>
        <span style="margin: 0 8px;">|</span>
        <span class="userinfo-action" @click="handleLogout" style="cursor:pointer; color:#409EFF;">登出</span>
      </template>
      <template v-else>
        <span style="color: #666;">由 Yao List 驱动</span>
        <span style="margin: 0 8px;">|</span>
        <span class="userinfo-action" @click="handleLogin" style="cursor:pointer; color:#409EFF;">登录</span>
      </template>
    </div>

    
    <!-- 重命名对话框 -->
    <Teleport to="body">
      <div v-if="renameDialog.show" class="dialog-overlay" @click="closeRenameDialog">
        <div class="dialog-container" @click.stop>
          <div class="dialog-header">
            <h3>重命名</h3>
            <button class="dialog-close" @click="closeRenameDialog">
              <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <line x1="18" y1="6" x2="6" y2="18"/>
                <line x1="6" y1="6" x2="18" y2="18"/>
              </svg>
            </button>
          </div>
          <div class="dialog-body">
            <p>请输入新的文件名：</p>
            <input 
              ref="renameInput"
              v-model="renameDialog.newName" 
              type="text" 
              class="dialog-input"
              @keyup.enter="confirmRename"
              @keyup.escape="closeRenameDialog"
            />
          </div>
          <div class="dialog-footer">
            <button class="dialog-btn cancel" @click="closeRenameDialog">取消</button>
            <button class="dialog-btn confirm" @click="confirmRename" :disabled="!renameDialog.newName.trim()">确定</button>
          </div>
        </div>
      </div>
    </Teleport>

    <!-- 创建文件夹对话框 -->
    <Teleport to="body">
      <div v-if="createFolderDialog.show" class="dialog-overlay" @click="closeCreateFolderDialog">
        <div class="dialog-container" @click.stop>
          <div class="dialog-header">
            <h3>创建文件夹</h3>
            <button class="dialog-close" @click="closeCreateFolderDialog">
              <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <line x1="18" y1="6" x2="6" y2="18"/>
                <line x1="6" y1="6" x2="18" y2="18"/>
              </svg>
            </button>
          </div>
          <div class="dialog-body">
            <p>请输入文件夹名称：</p>
            <input 
              ref="createFolderInput"
              v-model="createFolderDialog.folderName" 
              type="text" 
              class="dialog-input"
              placeholder="新建文件夹"
              @keyup.enter="confirmCreateFolder"
              @keyup.escape="closeCreateFolderDialog"
            />
          </div>
          <div class="dialog-footer">
            <button class="dialog-btn cancel" @click="closeCreateFolderDialog">取消</button>
            <button class="dialog-btn confirm" @click="confirmCreateFolder" :disabled="!createFolderDialog.folderName.trim()">创建</button>
          </div>
        </div>
      </div>
    </Teleport>

    <!-- 删除确认对话框 -->
    <Teleport to="body">
      <div v-if="deleteDialog.show" class="dialog-overlay" @click="closeDeleteDialog">
        <div class="dialog-container" @click.stop>
          <div class="dialog-header">
            <h3>确认删除</h3>
            <button class="dialog-close" @click="closeDeleteDialog">
              <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <line x1="18" y1="6" x2="6" y2="18"/>
                <line x1="6" y1="6" x2="18" y2="18"/>
              </svg>
            </button>
          </div>
          <div class="dialog-body">
            <div class="delete-warning">
              <svg width="48" height="48" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <path d="M10.29 3.86L1.82 18a2 2 0 001.71 3h16.94a2 2 0 001.71-3L13.71 3.86a2 2 0 00-3.42 0z"/>
                <line x1="12" y1="9" x2="12" y2="13"/>
                <line x1="12" y1="17" x2="12.01" y2="17"/>
              </svg>
            </div>
            <p v-if="deleteDialog.isBatch">{{ deleteDialog.message }}</p>
            <p v-else>确定要删除{{ deleteDialog.fileType }} <strong>"{{ deleteDialog.fileName }}"</strong> 吗？</p>
            <p class="warning-text">此操作不可恢复！</p>
          </div>
          <div class="dialog-footer">
            <button class="dialog-btn cancel" @click="closeDeleteDialog">取消</button>
            <button class="dialog-btn danger" @click="confirmDelete">删除</button>
          </div>
        </div>
      </div>
    </Teleport>

    <!-- 上传窗口 -->
    <Teleport to="body">
      <div v-if="uploadDialog.show" class="upload-overlay" @click="closeUploadDialog">
        <div class="upload-container" @click.stop>
          <div class="upload-header">
            <div class="upload-title-section">
              <h3>上传文件</h3>
              <div class="upload-path">
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                  <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/>
                </svg>
                {{ currentPath || '/' }}
              </div>
            </div>
            <button class="dialog-close" @click="closeUploadDialog">
              <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <line x1="18" y1="6" x2="6" y2="18"/>
                <line x1="6" y1="6" x2="18" y2="18"/>
              </svg>
            </button>
          </div>
          
          <!-- 拖拽区域 -->
          <div 
            class="upload-drop-zone"
            :class="{ 'drag-over': uploadDragOver }"
            @dragover.prevent="handleUploadDragOver"
            @dragleave.prevent="handleUploadDragLeave"
            @drop.prevent="handleUploadDrop"
            @click="triggerFileSelect"
          >
            <div class="upload-drop-content">
              <div class="upload-icon-container">
                <svg width="48" height="48" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                  <path d="M21 15v4a2 2 0 01-2 2H5a2 2 0 01-2-2v-4"/>
                  <polyline points="7,10 12,5 17,10"/>
                  <line x1="12" y1="5" x2="12" y2="15"/>
                </svg>
              </div>
              <h4>拖拽文件或文件夹到此处</h4>
              <div class="upload-options">
                <button class="upload-option-btn primary" @click="triggerFileSelect" @click.stop>
                  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                    <path d="M14 2H6a2 2 0 00-2 2v16a2 2 0 002 2h12a2 2 0 002-2V8z"/>
                    <polyline points="14,2 14,8 20,8"/>
                  </svg>
                  选择文件
                </button>
                <button class="upload-option-btn secondary" @click="triggerFolderSelect" @click.stop>
                  <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                    <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/>
                  </svg>
                  选择文件夹
                </button>
              </div>
              <p class="upload-hint">支持多文件选择和完整文件夹结构上传</p>
            </div>
          </div>
          
          <!-- 文件列表 -->
          <div v-if="uploadFiles.length > 0" class="upload-files">
            <div class="upload-files-header">
              <span>文件列表 ({{ uploadFiles.length }})</span>
              <button class="clear-btn" @click="clearUploadFiles">清空</button>
            </div>
            <div class="upload-file-list">
              <div v-for="(file, index) in uploadFiles" :key="index" class="upload-file-item">
                <div class="file-info">
                  <div class="file-icon" :class="{ 'folder-icon': file.relativePath && file.relativePath.includes('/') }">
                    <svg v-if="file.relativePath && file.relativePath.includes('/')" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                      <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/>
                    </svg>
                    <svg v-else width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                      <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/>
                      <polyline points="14,2 14,8 20,8"/>
                    </svg>
                  </div>
                  <div class="file-details">
                    <div class="file-name" :title="file.displayName">{{ file.displayName }}</div>
                    <div class="file-meta">
                      <span class="file-size">{{ formatFileSize(file.size) }}</span>
                      <span v-if="file.relativePath && file.relativePath !== file.name" class="file-path" :title="file.relativePath">
                        来自: {{ file.relativePath.split('/').slice(0, -1).join('/') || '根目录' }}
                      </span>
                    </div>
                  </div>
                </div>
                
                <div class="upload-progress">
                  <div class="progress-info">
                    <span class="progress-text">{{ getUploadStatusText(file) }}</span>
                    <span class="progress-speed" v-if="file.speed">{{ file.speed }}</span>
                  </div>
                  <div class="progress-bar">
                    <div 
                      class="progress-fill" 
                      :style="{ width: file.progress + '%' }"
                      :class="{ 
                        'progress-success': file.status === 'completed',
                        'progress-error': file.status === 'error'
                      }"
                    ></div>
                  </div>
                </div>
                
                <div class="file-actions">
                  <button 
                    v-if="file.status === 'uploading'" 
                    class="cancel-btn" 
                    @click="cancelUpload(index)"
                    title="取消上传"
                  >
                    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                      <circle cx="12" cy="12" r="10"/>
                      <line x1="15" y1="9" x2="9" y2="15"/>
                      <line x1="9" y1="9" x2="15" y2="15"/>
                    </svg>
                  </button>
                  <button 
                    v-if="file.status === 'pending' || file.status === 'error'" 
                    class="remove-btn" 
                    @click="removeUploadFile(index)"
                    title="移除文件"
                  >
                    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                      <line x1="18" y1="6" x2="6" y2="18"/>
                      <line x1="6" y1="6" x2="18" y2="18"/>
                    </svg>
                  </button>
                </div>
              </div>
            </div>
          </div>
          
          <!-- 操作按钮 -->
          <div class="upload-footer">
            <button class="dialog-btn cancel" @click="closeUploadDialog">
              {{ isUploading ? '关闭' : '取消' }}
            </button>
            <button 
              v-if="isUploading"
              class="dialog-btn danger" 
              @click="cancelAllUploads"
            >
              取消所有上传
            </button>
            <button 
              v-else
              class="dialog-btn confirm" 
              @click="startUpload" 
              :disabled="uploadFiles.length === 0"
            >
              开始上传
            </button>
          </div>
          
          <!-- 隐藏的文件输入 -->
          <input 
            ref="uploadFileInput" 
            type="file" 
            multiple 
            style="display: none" 
            @change="handleUploadFileSelect"
          />
          <!-- 隐藏的文件夹输入 -->
          <input 
            ref="uploadFolderInput" 
            type="file" 
            webkitdirectory="true"
            directory
            multiple 
            style="display: none" 
            @change="handleUploadFolderSelect"
          />
        </div>
      </div>
    </Teleport>

    <!-- 右键菜单 Teleport 到 body -->
    <Teleport to="body">
      <div
        v-if="contextMenu.show"
        class="custom-context-menu"
        :style="{ left: contextMenu.x + 'px', top: contextMenu.y + 'px', position: 'fixed', zIndex: 99999 }"
      >
        <!-- 文件管理菜单（根据权限显示） -->
        <template v-if="hasPermission(PERM_RENAME)">
          <div class="context-menu-item" @click="menuRename">
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
              <path d="M11 4H4a2 2 0 00-2 2v14a2 2 0 002 2h14a2 2 0 002-2v-7"/>
              <path d="M18.5 2.5a2.121 2.121 0 013 3L12 15l-4 1 1-4 9.5-9.5z"/>
            </svg>
            重命名
          </div>
        </template>
        
        <!-- 移动和复制功能 -->
        
        <template v-if="hasPermission(PERM_MOVE)">
          <div class="context-menu-item" @click="menuMove">
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
              <path d="M13 2L3 14h9l-1 8 10-12h-9l1-8z"/>
            </svg>
            移动
          </div>
        </template>
        
        <template v-if="hasPermission(PERM_COPY)">
          <div class="context-menu-item" @click="menuCopy">
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
              <rect x="9" y="9" width="13" height="13" rx="2" ry="2"/>
              <path d="M5 15H4a2 2 0 01-2-2V4a2 2 0 012-2h9a2 2 0 012 2v1"/>
            </svg>
            复制
          </div>
        </template>
        
        <template v-if="hasPermission(PERM_DELETE)">
          <div class="context-menu-item danger" @click="menuDelete">
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
              <polyline points="3,6 5,6 21,6"/>
              <path d="M19 6v14a2 2 0 01-2 2H7a2 2 0 01-2-2V6m3 0V4a2 2 0 012-2h4a2 2 0 012 2v2"/>
              <line x1="10" y1="11" x2="10" y2="17"/>
              <line x1="14" y1="11" x2="14" y2="17"/>
            </svg>
            删除
          </div>
        </template>
        
        <template v-if="hasPermission(PERM_RENAME) || hasPermission(PERM_DELETE)">
          <div class="context-menu-divider"></div>
        </template>
        
        <!-- 文件操作菜单（仅对文件显示，根据权限） -->
        <template v-if="!contextMenu.row?.is_dir && hasPermission(PERM_DOWNLOAD)">
          <div class="context-menu-item" @click="menuDownload">
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
              <path d="M21 15v4a2 2 0 01-2 2H5a2 2 0 01-2-2v-4"/>
              <polyline points="7,10 12,15 17,10"/>
              <line x1="12" y1="15" x2="12" y2="3"/>
            </svg>
            下载
          </div>
          <div class="context-menu-item" @click="menuCopyLink">
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
              <path d="M10 13a5 5 0 007.54.54l3-3a5 5 0 00-7.07-7.07l-1.72 1.71"/>
              <path d="M14 11a5 5 0 00-7.54-.54l-3 3a5 5 0 007.07 7.07l1.71-1.71"/>
            </svg>
            复制链接
          </div>
        </template>
        
        <!-- 预览菜单（仅对可预览文件显示，需要下载权限） -->
        <template v-if="!contextMenu.row?.is_dir && canPreview(contextMenu.row) && hasPermission(PERM_DOWNLOAD)">
          <div class="context-menu-item" @click="menuPreview">
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
              <path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z"/>
              <circle cx="12" cy="12" r="3"/>
            </svg>
            预览
          </div>
        </template>
      </div>
    </Teleport>

    <!-- 移动/复制对话框 -->
    <Teleport to="body">
      <div v-if="transferDialog.visible" class="dialog-overlay" @click="closeTransferDialog">
        <div class="dialog-container" @click.stop>
          <div class="dialog-header">
            <h3>{{ transferDialog.action === 'copy' ? '复制到' : '移动到' }}</h3>
            <button class="dialog-close" @click="closeTransferDialog">
              <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <line x1="18" y1="6" x2="6" y2="18"/>
                <line x1="6" y1="6" x2="18" y2="18"/>
              </svg>
            </button>
          </div>
          <div class="dialog-body">
            <div class="transfer-path">
              <span class="transfer-path-label">目标路径：</span>
              <span class="transfer-path-value">{{ transferDialog.currentPath }}</span>
            </div>
            <div class="transfer-dirs">
              <div v-if="transferDialog.currentPath !== '/'" class="transfer-dir-item" @click="transferGoUp">
                <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                  <path d="M19 12H5M12 19l-7-7 7-7"/>
                </svg>
                <span>返回上级目录</span>
              </div>
              <div 
                v-for="dir in transferDialog.dirs" 
                :key="dir.path"
                class="transfer-dir-item"
                @click="enterTransferDir(dir)"
              >
                <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                  <path d="M22 19a2 2 0 01-2 2H4a2 2 0 01-2-2V5a2 2 0 012-2h5l2 3h9a2 2 0 012 2z"/>
                </svg>
                <span>{{ dir.name }}</span>
              </div>
            </div>
          </div>
          <div class="dialog-footer">
            <button class="dialog-btn cancel" @click="closeTransferDialog">取消</button>
            <button class="dialog-btn confirm" @click="confirmTransfer">确定</button>
          </div>
        </div>
      </div>
    </Teleport>
  </div>
</template>

<script setup>
import { ref, computed, onMounted, onUnmounted, nextTick, Teleport, watch } from 'vue';
import { useRouter, useRoute } from 'vue-router';
// 移除Element Plus消息组件，使用自定义消息提示
// 移除Element Plus图标导入，使用自定义扁平化SVG图标
import axios from 'axios';
import FileDetail from './FileDetail.vue';
import notification from './utils/notification.js';
import { ElMessage } from 'element-plus';
import { ElMessageBox } from 'element-plus';
import { ElLoading } from 'element-plus';
import { Marked } from 'marked'
import DOMPurify from 'dompurify'

const marked = new Marked()
const router = useRouter();
const route = useRoute();

const files = ref([]);
const currentPath = ref('/');
const loading = ref(false);

// 判断是否为文件预览
const isFilePreview = computed(() => {
  const path = route.path;
  // 如果路径指向一个文件（有扩展名），则显示预览
  const lastSegment = path.split('/').pop();
  if (!lastSegment || !lastSegment.includes('.')) {
    return false;
  }
  
  // 只要路径包含文件扩展名，就认为是文件预览
  // FileDetail组件会处理是否支持预览的逻辑
  return true;
});
const currentPage = ref(1);
const pageSize = ref(20);
const contextMenu = ref({ show: false, x: 0, y: 0, row: null });
const user = ref({});
const isDarkMode = ref(localStorage.getItem('yaolist_dark_mode') === 'true');

// 对话框状态
const renameDialog = ref({
  show: false,
  file: null,
  newName: ''
});

const deleteDialog = ref({
  show: false,
  file: null,
  fileName: '',
  fileType: ''
});

const createFolderDialog = ref({
  show: false,
  folderName: ''
});

// 上传窗口相关数据
const uploadDialog = ref({
  show: false
});
const uploadFiles = ref([]);
const uploadFileInput = ref(null);
const uploadFolderInput = ref(null);
const uploadDragOver = ref(false);
const isUploading = ref(false);
const uploadAbortControllers = ref(new Map()); // 存储每个文件的取消控制器

// 悬浮菜单相关

const checkboxMode = ref(false);
const selectedFiles = ref([]);
const fileTable = ref(null);

const renameInput = ref(null);
const createFolderInput = ref(null);
const fileInput = ref(null);
const uploading = ref(false);
const dragOver = ref(false);
const siteInfo = ref({
  site_title: 'YaoList',
  site_description: '现代化的文件管理系统',
  theme_color: '#1976d2',
  site_icon: 'https://api.ylist.org/logo/logo.svg',
  favicon: 'https://api.ylist.org/logo/logo.svg',
  allow_registration: true,
  items_per_page: 20,
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
  bottom_message: '',
  enable_glass_effect: false
});





// 权限常量
const PERM_UPLOAD = 1 << 0; // 1 创建目录或上传
const PERM_DOWNLOAD = 1 << 1; // 2 下载(包括在线预览)
const PERM_DELETE = 1 << 2; // 4 删除
const PERM_COPY = 1 << 3; // 8 复制
const PERM_MOVE = 1 << 4; // 16 移动
const PERM_RENAME = 1 << 5; // 32 重命名
const PERM_LIST = 1 << 6; // 64 列表

// 权限检查函数
function hasPermission(permission) {
  return user.value.permissions && (user.value.permissions & permission) !== 0;
}

// 检查是否为管理员（admin用户名或拥有所有权限）
function isAdmin() {
  return user.value.username === 'admin' || 
         (user.value.permissions && user.value.permissions === (PERM_UPLOAD | PERM_DOWNLOAD | PERM_DELETE | PERM_COPY | PERM_MOVE | PERM_RENAME | PERM_LIST));
}

function onLogoError(e) {
  e.target.style.display = 'none';
}

// 根据文件扩展名获取扁平化图标和颜色
function getFileIcon(fileName, isDir) {
  if (isDir) {
    return { 
      icon: 'folder', 
      color: '#3b82f6',
      svg: `<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
        <path d="M3 7v10a2 2 0 002 2h14a2 2 0 002-2V9a2 2 0 00-2-2h-6l-2-2H5a2 2 0 00-2 2z"/>
      </svg>`
    };
  }
  
  const ext = fileName.split('.').pop()?.toLowerCase() || '';
  
  // 视频文件
  const videoExts = ['mp4', 'avi', 'mkv', 'mov', 'wmv', 'flv', 'webm', 'rmvb', 'm4v', '3gp'];
  if (videoExts.includes(ext)) {
    return { 
      icon: 'video', 
      color: '#f59e0b',
      svg: `<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
        <polygon points="23 7 16 12 23 17 23 7"/>
        <rect x="1" y="5" width="15" height="14" rx="2" ry="2"/>
      </svg>`
    };
  }
  
  // 音频文件
  const audioExts = ['mp3', 'wav', 'flac', 'aac', 'ogg', 'wma', 'm4a', 'opus'];
  if (audioExts.includes(ext)) {
    return { 
      icon: 'audio', 
      color: '#8b5cf6',
      svg: `<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
        <path d="M9 18V5l12-2v13"/>
        <circle cx="6" cy="18" r="3"/>
        <circle cx="18" cy="16" r="3"/>
      </svg>`
    };
  }
  
  // 图片文件
  const imageExts = ['jpg', 'jpeg', 'png', 'gif', 'bmp', 'svg', 'ico', 'webp', 'tiff', 'tif'];
  if (imageExts.includes(ext)) {
    return { 
      icon: 'image', 
      color: '#10b981',
      svg: `<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
        <rect x="3" y="3" width="18" height="18" rx="2" ry="2"/>
        <circle cx="8.5" cy="8.5" r="1.5"/>
        <polyline points="21,15 16,10 5,21"/>
      </svg>`
    };
  }
  
  // 代码文件
  const codeExts = ['js', 'ts', 'jsx', 'tsx', 'vue', 'html', 'htm', 'css', 'scss', 'sass', 'less', 'php', 'py', 'java', 'c', 'cpp', 'h', 'hpp', 'cs', 'go', 'rs', 'rb', 'swift', 'kt'];
  if (codeExts.includes(ext)) {
    return { 
      icon: 'code', 
      color: '#ef4444',
      svg: `<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
        <polyline points="16,18 22,12 16,6"/>
        <polyline points="8,6 2,12 8,18"/>
      </svg>`
    };
  }
  
  // 数据文件
  const dataExts = ['json', 'xml', 'yaml', 'yml', 'csv', 'sql', 'db', 'sqlite'];
  if (dataExts.includes(ext)) {
    return { 
      icon: 'database', 
      color: '#f59e0b',
      svg: `<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
        <ellipse cx="12" cy="5" rx="9" ry="3"/>
        <path d="M21 12c0 1.66-4 3-9 3s-9-1.34-9-3"/>
        <path d="M3 5v14c0 1.66 4 3 9 3s9-1.34 9-3V5"/>
      </svg>`
    };
  }
  
  // 文档文件
  const docExts = ['pdf', 'doc', 'docx', 'xls', 'xlsx', 'ppt', 'pptx', 'txt', 'md', 'rtf'];
  if (docExts.includes(ext)) {
    return { 
      icon: 'document', 
      color: '#3b82f6',
      svg: `<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
        <path d="M14 2H6a2 2 0 00-2 2v16a2 2 0 002 2h12a2 2 0 002-2V8z"/>
        <polyline points="14,2 14,8 20,8"/>
        <line x1="16" y1="13" x2="8" y2="13"/>
        <line x1="16" y1="17" x2="8" y2="17"/>
        <polyline points="10,9 9,9 8,9"/>
      </svg>`
    };
  }
  
  // 压缩文件
  const archiveExts = ['zip', 'rar', '7z', 'tar', 'gz', 'bz2', 'xz', 'iso'];
  if (archiveExts.includes(ext)) {
    return { 
      icon: 'archive', 
      color: '#6b7280',
      svg: `<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
        <path d="M21 8a2 2 0 00-1-1.73l-7-4a2 2 0 00-2 0l-7 4A2 2 0 003 8v8a2 2 0 001 1.73l7 4a2 2 0 002 0l7-4A2 2 0 0021 16V8z"/>
        <polyline points="7.5,4.21 12,6.81 16.5,4.21"/>
        <polyline points="7.5,19.79 7.5,14.6 3,12"/>
        <polyline points="21,12 16.5,14.6 16.5,19.79"/>
        <polyline points="3.27,6.96 12,12.01 20.73,6.96"/>
        <line x1="12" y1="22.08" x2="12" y2="12"/>
      </svg>`
    };
  }
  
  // 配置文件
  const configExts = ['conf', 'config', 'ini', 'cfg', 'properties', 'env', 'toml'];
  if (configExts.includes(ext)) {
    return { 
      icon: 'settings', 
      color: '#6b7280',
      svg: `<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
        <circle cx="12" cy="12" r="3"/>
        <path d="M19.4 15a1.65 1.65 0 00.33 1.82l.06.06a2 2 0 010 2.83 2 2 0 01-2.83 0l-.06-.06a1.65 1.65 0 00-1.82-.33 1.65 1.65 0 00-1 1.51V21a2 2 0 01-2 2 2 2 0 01-2-2v-.09A1.65 1.65 0 009 19.4a1.65 1.65 0 00-1.82.33l-.06.06a2 2 0 01-2.83 0 2 2 0 010-2.83l.06-.06a1.65 1.65 0 00.33-1.82 1.65 1.65 0 00-1.51-1H3a2 2 0 01-2-2 2 2 0 012-2h.09A1.65 1.65 0 004.6 9a1.65 1.65 0 00-.33-1.82l-.06-.06a2 2 0 010-2.83 2 2 0 012.83 0l.06.06a1.65 1.65 0 001.82.33H9a1.65 1.65 0 001-1.51V3a2 2 0 012-2 2 2 0 012 2v.09a1.65 1.65 0 001 1.51 1.65 1.65 0 001.82-.33l.06-.06a2 2 0 012.83 0 2 2 0 010 2.83l-.06.06a1.65 1.65 0 00-.33 1.82V9a1.65 1.65 0 001.51 1H21a2 2 0 012 2 2 2 0 01-2 2h-.09a1.65 1.65 0 00-1.51 1z"/>
      </svg>`
    };
  }
  
  // 可执行文件
  const execExts = ['exe', 'msi', 'dmg', 'pkg', 'deb', 'rpm', 'app', 'apk'];
  if (execExts.includes(ext)) {
    return { 
      icon: 'executable', 
      color: '#ef4444',
      svg: `<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
        <rect x="4" y="4" width="16" height="16" rx="2"/>
        <rect x="9" y="9" width="6" height="6"/>
        <line x1="9" y1="1" x2="9" y2="4"/>
        <line x1="15" y1="1" x2="15" y2="4"/>
        <line x1="9" y1="20" x2="9" y2="23"/>
        <line x1="15" y1="20" x2="15" y2="23"/>
        <line x1="20" y1="9" x2="23" y2="9"/>
        <line x1="20" y1="14" x2="23" y2="14"/>
        <line x1="1" y1="9" x2="4" y2="9"/>
        <line x1="1" y1="14" x2="4" y2="14"/>
      </svg>`
    };
  }
  
  // 字体文件
  const fontExts = ['ttf', 'otf', 'woff', 'woff2', 'eot'];
  if (fontExts.includes(ext)) {
    return { 
      icon: 'font', 
      color: '#6b7280',
      svg: `<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
        <polyline points="4,7 4,4 20,4 20,7"/>
        <line x1="9" y1="20" x2="15" y2="20"/>
        <line x1="12" y1="4" x2="12" y2="20"/>
      </svg>`
    };
  }
  
  // 默认文件图标
  return { 
    icon: 'file', 
    color: '#6b7280',
    svg: `<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
      <path d="M14 2H6a2 2 0 00-2 2v16a2 2 0 002 2h12a2 2 0 002-2V8z"/>
      <polyline points="14,2 14,8 20,8"/>
    </svg>`
  };
}

// 辅助函数：将实际路径转换为显示路径
function actualPathToDisplayPath(actualPath) {
  console.log('actualPathToDisplayPath - 输入:', actualPath);
  console.log('actualPathToDisplayPath - 用户路径:', user.value.user_path);
  
  if (!user.value.user_path || user.value.user_path === '/') {
    console.log('actualPathToDisplayPath - 无用户路径限制，返回原路径');
    return actualPath;
  }
  
  const userBasePath = user.value.user_path.replace(/\/+$/, '');
  console.log('actualPathToDisplayPath - 用户基础路径:', userBasePath);
  
  // 处理重复的用户路径前缀（如 /Onedrive/Onedrive/Desktop）
  let cleanPath = actualPath;
  
  // 如果路径以用户路径开头，移除第一个用户路径前缀
  if (cleanPath.startsWith(userBasePath)) {
    cleanPath = cleanPath.substring(userBasePath.length);
    if (!cleanPath.startsWith('/')) {
      cleanPath = '/' + cleanPath;
    }
    console.log('actualPathToDisplayPath - 第一次移除后:', cleanPath);
  }
  
  // 如果还是以用户路径开头（说明有重复），再次移除
  if (cleanPath.startsWith(userBasePath)) {
    cleanPath = cleanPath.substring(userBasePath.length);
    if (!cleanPath.startsWith('/')) {
      cleanPath = '/' + cleanPath;
    }
    console.log('actualPathToDisplayPath - 第二次移除后:', cleanPath);
  }
  
  console.log('actualPathToDisplayPath - 最终结果:', cleanPath);
  return cleanPath;
}

// 辅助函数：将显示路径转换为实际路径
function displayPathToActualPath(displayPath) {
  if (!user.value.user_path || user.value.user_path === '/') {
    return displayPath;
  }
  
  const userBasePath = user.value.user_path.replace(/\/+$/, '');
  if (displayPath === '/') {
    return userBasePath;
  } else {
    return userBasePath + displayPath;
  }
}

const pathBreadcrumbs = computed(() => {
  // 面包屑应该基于当前的URL路径（用户看到的路径）
  let displayPath = decodeURIComponent(route.path).replace(/\\/g, '/');
  
  const parts = displayPath.split('/').filter(Boolean);
  const crumbs = [{ name: '🏠主页', path: '/' }];
  let path = '';
  for (const part of parts) {
    path += '/' + part;
    crumbs.push({ name: part, path });
  }
  return crumbs;
});

async function fetchFiles(path = '/') {
  loading.value = true;
  try {
    // 确保路径以/开头且不以/结尾（除了根路径）
    path = path.replace(/\\/g, '/').replace(/\/+/g, '/');
    if (!path.startsWith('/')) path = '/' + path;
    if (path !== '/' && path.endsWith('/')) path = path.slice(0, -1);
    
    const res = await axios.get('/api/files', { 
      params: { path }
    });
    files.value = res.data;
  } catch (e) {
    console.error('获取文件列表失败:', e);
    files.value = [];
  } finally {
    loading.value = false;
  }
}

function getRelPath(path) {
  let rel = path.replace(/^([A-Za-z]:)?[\\/]+/, '');
  rel = rel.replace(/^Yaolist[\\/]/, '');
  return rel;
}

function handleRowClick(row) {
  if (row.is_dir) {
    // 后端返回的row.path是实际路径，需要转换为显示路径
    const displayPath = actualPathToDisplayPath(row.path);
    navigateTo(displayPath);
  }
}

function navigateTo(displayPath, refresh = false) {
  // 清理显示路径
  displayPath = displayPath.replace(/\\/g, '/').replace(/\/+/g, '/');
  if (!displayPath.startsWith('/')) displayPath = '/' + displayPath;
  if (displayPath !== '/' && displayPath.endsWith('/')) displayPath = displayPath.slice(0, -1);
  
  // 对显示路径进行URL编码，但保留路径分隔符
  const encodedPath = displayPath.split('/').map(segment => segment ? encodeURIComponent(segment) : '').join('/');
  
  router.push(encodedPath);
  if (refresh) {
    // 将显示路径转换为实际路径
    const actualPath = displayPathToActualPath(displayPath);
    currentPath.value = actualPath;
    fetchFiles(actualPath);
  }
}



function formatDate(date) {
  if (!date) return '-';
  const d = typeof date === 'string' ? new Date(date) : date;
  if (isNaN(d.getTime())) return '-';
  const pad = n => n.toString().padStart(2, '0');
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
}

function handleLogin() {
  // 如果当前是游客用户，先清除登录状态
  if (user.value.username === 'guest') {
    user.value = {};
  }
  router.push('/login');
}
function handleRegister() {
  router.push('/register');
}
function handleLogout() {
  // 调用登出API
  axios.post('/api/logout').then(() => {
    user.value = {};
    notification.success('已成功登出');
    setTimeout(() => {
      router.push('/login');
    }, 1000);
  }).catch(() => {
    // 即使API调用失败，也清除本地状态
    user.value = {};
    notification.success('已成功登出');
    setTimeout(() => {
      router.push('/login');
    }, 1000);
  });
}

// 切换日夜模式
function toggleDarkMode() {
  isDarkMode.value = !isDarkMode.value;
  localStorage.setItem('yaolist_dark_mode', isDarkMode.value.toString());
  
  // 应用主题到body
  if (isDarkMode.value) {
    document.body.classList.add('dark-mode');
  } else {
    document.body.classList.remove('dark-mode');
  }
}

const paginatedFiles = computed(() => {
  const start = (currentPage.value - 1) * pageSize.value;
  return files.value.slice(start, start + pageSize.value);
});

function handlePageChange(page) {
  currentPage.value = page;
}

function downloadFile(row) {
  // 将实际路径转换为显示路径
  const displayPath = actualPathToDisplayPath(row.path);
  const path = encodeURIComponent(displayPath);
  const downloadUrl = `/api/download?path=${path}`;
  window.open(downloadUrl);
  notification.success('开始下载文件');
}

function copyLink(row) {
  // 将实际路径转换为显示路径
  const displayPath = actualPathToDisplayPath(row.path);
  const path = encodeURIComponent(displayPath);
  const link = `${window.location.origin}/api/download?path=${path}`;
  navigator.clipboard.writeText(link).then(() => {
    notification.success('链接已复制到剪贴板');
  }).catch(() => {
    // 降级方案
    const textArea = document.createElement('textarea');
    textArea.value = link;
    document.body.appendChild(textArea);
    textArea.select();
    document.execCommand('copy');
    document.body.removeChild(textArea);
    notification.success('链接已复制到剪贴板');
  });
}

function canPreview(file) {
  if (file.is_dir) return false;
  
  const ext = getFileExtension(file.name).toLowerCase();
  
  const textTypes = siteInfo.value.preview_text_types.split(',').map(t => t.trim());
  const audioTypes = siteInfo.value.preview_audio_types.split(',').map(t => t.trim());
  const videoTypes = siteInfo.value.preview_video_types.split(',').map(t => t.trim());
  const imageTypes = siteInfo.value.preview_image_types.split(',').map(t => t.trim());
  const proxyTypes = siteInfo.value.preview_proxy_types.split(',').map(t => t.trim());
  
  // 解析外部预览配置
  let externalConfig = {};
  try {
    externalConfig = JSON.parse(siteInfo.value.preview_external);
  } catch (e) {
    console.error('解析外部预览配置失败:', e);
  }
  
  // 检查是否在外部预览配置中
  for (const types in externalConfig) {
    if (types.split(',').map(t => t.trim()).includes(ext)) {
      return 'external';
    }
  }
  
  // 解析iframe配置
  let iframeConfig = {};
  try {
    iframeConfig = JSON.parse(siteInfo.value.preview_iframe);
  } catch (e) {
    console.error('解析iframe配置失败:', e);
  }
  
  // 检查是否在iframe配置中
  for (const types in iframeConfig) {
    if (types.split(',').map(t => t.trim()).includes(ext)) {
      return 'iframe';
    }
  }
  
  if (textTypes.includes(ext)) {
    return 'text';
  }
  if (audioTypes.includes(ext)) {
    return 'audio';
  }
  if (videoTypes.includes(ext)) {
    return 'video';
  }
  if (imageTypes.includes(ext)) {
    return 'image';
  }
  if (proxyTypes.includes(ext)) {
    return 'proxy';
  }
  
  return false;
}

function getFileExtension(filename) {
  return filename.split('.').pop() || '';
}

// 处理文件点击事件
function handleFileClick(row) {
  if (row.is_dir) {
    handleRowClick(row);
  } else {
    // 对于文件，使用row.path，如果不存在则构建完整路径
    let actualFilePath;
    if (row.path) {
      actualFilePath = row.path;
    } else {
      // 构建完整路径
      const currentDir = currentPath.value.endsWith('/') ? currentPath.value : currentPath.value + '/';
      actualFilePath = currentDir + row.name;
    }
    
    // 将实际路径转换为显示路径
    const displayPath = actualPathToDisplayPath(actualFilePath);
    
    // 清理路径：移除双斜杠，确保路径格式正确
    const cleanDisplayPath = displayPath.replace(/\\/g, '/').replace(/\/+/g, '/');
    
    // 直接跳转到FileDetail页面，使用显示路径作为路由
    // 对路径进行URL编码，但保留路径分隔符
    const encodedFilePath = cleanDisplayPath.split('/').map(segment => segment ? encodeURIComponent(segment) : '').join('/');
    router.push(encodedFilePath);
  }
}

function onPreviewError() {
  preview.value.type = 'error';
  preview.value.error = '加载失败';
}

function onImageLoad() {
  // 可以在这里添加图片加载完成的处理逻辑
}

// 视频加载完成事件处理
function onVideoLoaded(event) {
  const video = event.target;
  // 强制显示控制条
  video.controls = true;
  video.setAttribute('controls', 'controls');
  
  // 确保控制条可见
  setTimeout(() => {
    video.style.setProperty('-webkit-appearance', 'media-controls-background', 'important');
    video.style.setProperty('appearance', 'none', 'important');
    
    // 强制重新渲染控制条
    const display = video.style.display;
    video.style.display = 'none';
    video.offsetHeight; // 触发重排
    video.style.display = display;
  }, 100);
}

function downloadTextFile() {
  downloadFile(preview.value.file);
}

function closePreview() {
  preview.value.show = false;
  preview.value.type = '';
  preview.value.url = '';
  preview.value.content = '';
  textLanguage.value = '';
  selectedOfficeViewer.value = '';
}

function handleRowContextMenu(row, column, event) {
  event.preventDefault();
  contextMenu.value = {
    show: true,
    x: event.clientX,
    y: event.clientY,
    row
  };
  nextTick(() => {
    window.addEventListener('mousedown', closeContextMenu, { once: true });
  });
}

function closeContextMenu(e) {
  if (!e || !e.target.closest('.custom-context-menu')) {
    contextMenu.value.show = false;
  }
}

function menuDownload() {
  if (contextMenu.value.row) downloadFile(contextMenu.value.row);
  contextMenu.value.show = false;
}

function menuCopyLink() {
  if (contextMenu.value.row) copyLink(contextMenu.value.row);
  contextMenu.value.show = false;
}

function menuPreview() {
  if (contextMenu.value.row) handleFileClick(contextMenu.value.row);
  contextMenu.value.show = false;
}

function menuRename() {
  if (contextMenu.value.row) {
    renameDialog.value = {
      show: true,
      file: contextMenu.value.row,
      newName: contextMenu.value.row.name
    };
    // 延迟聚焦输入框
    nextTick(() => {
      if (renameInput.value) {
        renameInput.value.focus();
        renameInput.value.select();
      }
    });
  }
  contextMenu.value.show = false;
}

/*function menuMove() {
  notification.info('移动功能正在开发中...');
  contextMenu.value.show = false;
}

function menuCopy() {
  notification.info('复制功能正在开发中...');
  contextMenu.value.show = false;
}
*/
function menuDelete() {
  if (contextMenu.value.row) {
    deleteDialog.value = {
      show: true,
      file: contextMenu.value.row,
      fileName: contextMenu.value.row.name,
      fileType: contextMenu.value.row.is_dir ? '文件夹' : '文件'
    };
  }
  contextMenu.value.show = false;
}

// 对话框处理函数
function closeRenameDialog() {
  renameDialog.value.show = false;
  renameDialog.value.file = null;
  renameDialog.value.newName = '';
}

function confirmRename() {
  const newName = renameDialog.value.newName.trim();
  if (newName && newName !== renameDialog.value.file.name) {
    renameFile(renameDialog.value.file, newName);
  }
  closeRenameDialog();
}

function closeDeleteDialog() {
  deleteDialog.value.show = false;
  deleteDialog.value.file = null;
  deleteDialog.value.fileName = '';
  deleteDialog.value.fileType = '';
  deleteDialog.value.isBatch = false;
  deleteDialog.value.batchFiles = [];
  deleteDialog.value.message = '';
}

async function confirmDelete() {
  if (deleteDialog.value.isBatch && deleteDialog.value.batchFiles) {
    // 批量删除
    await batchDeleteFiles(deleteDialog.value.batchFiles);
  } else if (deleteDialog.value.file) {
    // 单个删除
    await deleteFile(deleteDialog.value.file);
  }
  closeDeleteDialog();
}

async function batchDeleteFiles(files) {
  let successCount = 0;
  let errorCount = 0;
  
  for (const file of files) {
    try {
      await deleteFile(file, false); // 不显示单个成功消息
      successCount++;
    } catch (error) {
      errorCount++;
    }
  }
  
  // 显示批量删除结果
  if (successCount > 0) {
    notification.success(`成功删除 ${successCount} 个项目`);
  }
  if (errorCount > 0) {
    notification.error(`${errorCount} 个项目删除失败`);
  }
  
  // 清空选择
  clearSelection();
  
  // 刷新文件列表
  fetchFiles(currentPath.value);
}

// 上传窗口相关方法
function openUploadDialog() {
  uploadDialog.value.show = true;
  uploadFiles.value = [];
  
  // 确保文件夹输入框的属性正确设置
  nextTick(() => {
    if (uploadFolderInput.value) {
      uploadFolderInput.value.setAttribute('webkitdirectory', 'true');
      uploadFolderInput.value.setAttribute('directory', 'true');
    }
  });
}

function closeUploadDialog() {
  uploadDialog.value.show = false;
  uploadFiles.value = [];
  isUploading.value = false;
  // 清理所有取消控制器
  uploadAbortControllers.value.clear();
}

function triggerFileSelect() {
  if (uploadFileInput.value) {
    uploadFileInput.value.click();
  }
}

function triggerFolderSelect() {
  if (!uploadFolderInput.value) {
    notification.error('文件夹选择功能初始化失败');
    return;
  }
  
  // 检查浏览器是否支持文件夹选择
  if (!('webkitdirectory' in uploadFolderInput.value)) {
    notification.error('您的浏览器不支持文件夹选择功能，请使用拖拽方式上传文件夹');
    return;
  }
  
  try {
    uploadFolderInput.value.click();
  } catch (error) {
    console.error('Failed to trigger folder select:', error);
    notification.error('无法打开文件夹选择对话框');
  }
}

function handleUploadFileSelect(event) {
  const files = Array.from(event.target.files || []);
  addUploadFiles(files);
  // 清空input
  if (uploadFileInput.value) {
    uploadFileInput.value.value = '';
  }
}

function handleUploadFolderSelect(event) {
  const files = Array.from(event.target.files || []);
  
  if (files.length === 0) {
    return;
  }
  
  // 为文件夹中的文件添加相对路径信息
  files.forEach(file => {
    if (file.webkitRelativePath) {
      file.relativePath = file.webkitRelativePath;
    }
  });
  
  addUploadFiles(files);
  
  // 清空input
  if (uploadFolderInput.value) {
    uploadFolderInput.value.value = '';
  }
}

function handleUploadDragOver(event) {
  event.preventDefault();
  uploadDragOver.value = true;
}

function handleUploadDragLeave(event) {
  event.preventDefault();
  uploadDragOver.value = false;
}

async function handleUploadDrop(event) {
  event.preventDefault();
  uploadDragOver.value = false;
  
  const items = Array.from(event.dataTransfer.items || []);
  const files = [];
  
  // 处理拖拽的项目（可能包含文件夹）
  for (const item of items) {
    if (item.kind === 'file') {
      const entry = item.webkitGetAsEntry();
      if (entry) {
        await processEntry(entry, files);
      }
    }
  }
  
  if (files.length > 0) {
    addUploadFiles(files);
  }
}

// 递归处理文件夹条目
async function processEntry(entry, files, path = '') {
  if (entry.isFile) {
    // 处理文件
    const file = await new Promise((resolve) => {
      entry.file(resolve);
    });
    // 保存相对路径信息
    file.relativePath = path + file.name;
    files.push(file);
  } else if (entry.isDirectory) {
    // 处理文件夹
    const reader = entry.createReader();
    const entries = await new Promise((resolve) => {
      reader.readEntries(resolve);
    });
    
    for (const childEntry of entries) {
      await processEntry(childEntry, files, path + entry.name + '/');
    }
  }
}

function addUploadFiles(files) {
  for (const file of files) {
    // 使用相对路径作为唯一标识，如果没有则使用文件名
    const displayName = file.relativePath || file.name;
    const uniqueKey = displayName + '_' + file.size;
    
    // 检查是否已存在
    const exists = uploadFiles.value.some(f => (f.displayName + '_' + f.size) === uniqueKey);
    if (!exists) {
      uploadFiles.value.push({
        file: file,
        name: file.name,
        displayName: displayName,
        relativePath: file.relativePath || '',
        size: file.size,
        progress: 0,
        status: 'pending', // pending, uploading, completed, error
        speed: '',
        error: ''
      });
    }
  }
}

function removeUploadFile(index) {
  uploadFiles.value.splice(index, 1);
}

function clearUploadFiles() {
  uploadFiles.value = [];
}

function formatFileSize(bytes) {
  if (!bytes || bytes === 0) return '0 B';
  const k = 1024;
  const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  
  // 限制最大单位到TB
  const index = Math.min(i, sizes.length - 1);
  const value = bytes / Math.pow(k, index);
  
  // 根据大小调整小数位数
  let decimals;
  if (value >= 100) {
    decimals = 0; // 100+ 不显示小数
  } else if (value >= 10) {
    decimals = 1; // 10-99 显示1位小数
  } else {
    decimals = 2; // <10 显示2位小数
  }
  
  return value.toFixed(decimals) + ' ' + sizes[index];
}

function getUploadStatusText(file) {
  switch (file.status) {
    case 'pending': return '等待上传';
    case 'uploading': return `上传中 ${file.progress}%`;
    case 'completed': return '上传完成';
    case 'error': return '上传失败';
    default: return '未知状态';
  }
}

async function startUpload() {
  if (uploadFiles.value.length === 0 || isUploading.value) return;
  
  isUploading.value = true;
  let successCount = 0;
  let errorCount = 0;
  let cancelCount = 0;
  
  for (const fileItem of uploadFiles.value) {
    if (fileItem.status === 'completed') continue;
    
    try {
      fileItem.status = 'uploading';
      fileItem.progress = 0;
      
      await uploadSingleFile(fileItem);
      
      fileItem.status = 'completed';
      fileItem.progress = 100;
      successCount++;
    } catch (error) {
      if (error.name === 'CanceledError' || error.code === 'ERR_CANCELED') {
        fileItem.status = 'error';
        fileItem.error = '用户取消';
        cancelCount++;
      } else {
        fileItem.status = 'error';
        fileItem.error = error.message;
        errorCount++;
      }
    }
  }
  
  isUploading.value = false;
  
  // 显示结果
  if (successCount > 0) {
    notification.success(`成功上传 ${successCount} 个文件`);
    // 刷新文件列表
    fetchFiles(currentPath.value);
  }
  
  if (errorCount > 0) {
    notification.error(`${errorCount} 个文件上传失败`);
  }
  
  if (cancelCount > 0) {
    notification.warning(`${cancelCount} 个文件被取消上传`);
  }
  
  // 如果全部成功，关闭窗口
  if (errorCount === 0 && cancelCount === 0) {
    setTimeout(() => {
      closeUploadDialog();
    }, 1000);
  }
}

async function uploadSingleFile(fileItem) {
  const formData = new FormData();
  formData.append('file', fileItem.file);
  formData.append('path', currentPath.value);
  
  // 如果有相对路径，传递给后端
  if (fileItem.relativePath) {
    formData.append('relative_path', fileItem.relativePath);
  }
  
  const startTime = Date.now();
  let lastTime = startTime;
  let lastLoaded = 0;
  
  // 创建取消控制器
  const abortController = new AbortController();
  const fileIndex = uploadFiles.value.indexOf(fileItem);
  uploadAbortControllers.value.set(fileIndex, abortController);
  
  try {
    await axios.post('/api/upload', formData, {
      headers: {
        'Content-Type': 'multipart/form-data'
      },
      signal: abortController.signal,
      timeout: 300000, // 5分钟超时
      onUploadProgress: (progressEvent) => {
        const percentCompleted = Math.round((progressEvent.loaded * 100) / progressEvent.total);
        fileItem.progress = percentCompleted;
        
        // 计算上传速度 - 使用更准确的时间间隔
        const currentTime = Date.now();
        const timeElapsed = (currentTime - lastTime) / 1000; // 秒
        const bytesUploaded = progressEvent.loaded - lastLoaded;
        
        // 只有当时间间隔大于0.5秒时才更新速度，避免频繁更新
        if (timeElapsed >= 0.5 && bytesUploaded > 0) {
          const speed = bytesUploaded / timeElapsed; // bytes per second
          
          if (speed > 1024 * 1024) {
            fileItem.speed = (speed / (1024 * 1024)).toFixed(1) + ' MB/s';
          } else if (speed > 1024) {
            fileItem.speed = (speed / 1024).toFixed(1) + ' KB/s';
          } else {
            fileItem.speed = Math.round(speed) + ' B/s';
          }
          
          // 更新基准时间和数据量
          lastTime = currentTime;
          lastLoaded = progressEvent.loaded;
        }
      }
    });
  } finally {
    // 清理取消控制器
    uploadAbortControllers.value.delete(fileIndex);
  }
}

// 取消单个文件上传
function cancelUpload(index) {
  const abortController = uploadAbortControllers.value.get(index);
  if (abortController) {
    abortController.abort();
    uploadFiles.value[index].status = 'error';
    uploadFiles.value[index].error = '用户取消';
    uploadAbortControllers.value.delete(index);
  }
}

// 取消所有上传
function cancelAllUploads() {
  uploadAbortControllers.value.forEach((controller, index) => {
    controller.abort();
    if (uploadFiles.value[index]) {
      uploadFiles.value[index].status = 'error';
      uploadFiles.value[index].error = '用户取消';
    }
  });
  uploadAbortControllers.value.clear();
  isUploading.value = false;
}

// 悬浮菜单相关方法
async function refreshFiles() {
  loading.value = true;
  try {
    const res = await axios.get('/api/files', { 
      params: { path: currentPath.value }
    });
    files.value = res.data;
    notification.success('文件列表刷新成功');
  } catch (error) {
    files.value = [];
    notification.error('刷新失败: ' + (error.response?.data || error.message || '未知错误'));
  } finally {
    loading.value = false;
  }
}

function toggleCheckboxMode() {
  checkboxMode.value = !checkboxMode.value;
  if (!checkboxMode.value) {
    // 退出选择模式时清空选择
    selectedFiles.value = [];
    if (fileTable.value) {
      fileTable.value.clearSelection();
    }
  }
}

function handleSelectionChange(selection) {
  selectedFiles.value = selection;
}

function clearSelection() {
  selectedFiles.value = [];
  if (fileTable.value) {
    fileTable.value.clearSelection();
  }
}

function batchDelete() {
  if (selectedFiles.value.length === 0) return;
  
  const fileCount = selectedFiles.value.filter(f => !f.is_dir).length;
  const folderCount = selectedFiles.value.filter(f => f.is_dir).length;
  
  let message = '确定要删除';
  if (fileCount > 0 && folderCount > 0) {
    message += ` ${fileCount} 个文件和 ${folderCount} 个文件夹`;
  } else if (fileCount > 0) {
    message += ` ${fileCount} 个文件`;
  } else {
    message += ` ${folderCount} 个文件夹`;
  }
  message += ' 吗？';
  
  deleteDialog.value = {
    show: true,
    file: null,
    fileName: '',
    fileType: '',
    isBatch: true,
    batchFiles: selectedFiles.value,
    message: message
  };
}

function openCreateFolderDialog() {
  createFolderDialog.value.show = true;
  createFolderDialog.value.folderName = '';
  // 聚焦到输入框
  nextTick(() => {
    if (createFolderInput.value) {
      createFolderInput.value.focus();
    }
  });
}

function closeCreateFolderDialog() {
  createFolderDialog.value.show = false;
  createFolderDialog.value.folderName = '';
}

async function confirmCreateFolder() {
  const folderName = createFolderDialog.value.folderName.trim();
  if (!folderName) return;
  
  try {
    await createFolder(folderName);
    closeCreateFolderDialog();
  } catch (error) {
    // 错误已在createFolder函数中处理
  }
}

async function createFolder(folderName) {
  try {
    await axios.post('/api/create-folder', {
      parent_path: currentPath.value,
      folder_name: folderName
    });
    
    notification.success('文件夹创建成功');
    
    // 延迟一下再刷新，确保后端操作完成
    setTimeout(() => {
      fetchFiles(currentPath.value);
    }, 500);
  } catch (error) {
    notification.error(error.response?.data || '创建文件夹失败');
    throw error;
  }
}

// 上传功能
function triggerFileUpload() {
  if (fileInput.value) {
    fileInput.value.click();
  }
}

async function handleFileSelect(event) {
  const files = event.target.files;
  if (!files || files.length === 0) return;
  
  uploading.value = true;
  
  try {
    for (let i = 0; i < files.length; i++) {
      await uploadFile(files[i]);
    }
    notification.success(`成功上传 ${files.length} 个文件`);
    // 刷新文件列表
    fetchFiles(currentPath.value);
  } catch (error) {
    notification.error('上传失败: ' + (error.response?.data || error.message));
  } finally {
    uploading.value = false;
    // 清空文件选择
    if (fileInput.value) {
      fileInput.value.value = '';
    }
  }
}

async function uploadFile(file) {
  const formData = new FormData();
  formData.append('file', file);
  formData.append('path', currentPath.value);
  
  // 如果有相对路径，传递给后端
  if (file.relativePath) {
    formData.append('relative_path', file.relativePath);
  }
  
  await axios.post('/api/upload', formData, {
    headers: {
      'Content-Type': 'multipart/form-data'
    },
    onUploadProgress: (progressEvent) => {
      // 可以在这里添加上传进度显示
      const percentCompleted = Math.round((progressEvent.loaded * 100) / progressEvent.total);
      console.log(`上传进度: ${percentCompleted}%`);
    }
  });
}

// 拖拽上传功能
function handleDragOver(event) {
  if (!hasPermission(PERM_UPLOAD)) return;
  event.preventDefault();
  dragOver.value = true;
}

function handleDragLeave(event) {
  if (!hasPermission(PERM_UPLOAD)) return;
  event.preventDefault();
  // 检查是否真的离开了拖拽区域
  const rect = event.currentTarget.getBoundingClientRect();
  const x = event.clientX;
  const y = event.clientY;
  
  if (x < rect.left || x > rect.right || y < rect.top || y > rect.bottom) {
    dragOver.value = false;
  }
}

async function handleDrop(event) {
  if (!hasPermission(PERM_UPLOAD)) return;
  event.preventDefault();
  dragOver.value = false;
  
  const files = Array.from(event.dataTransfer.files);
  if (files.length === 0) return;
  
  uploading.value = true;
  
  try {
    for (let i = 0; i < files.length; i++) {
      await uploadFile(files[i]);
    }
    notification.success(`成功上传 ${files.length} 个文件`);
    // 刷新文件列表
    fetchFiles(currentPath.value);
  } catch (error) {
    notification.error('上传失败: ' + (error.response?.data || error.message));
  } finally {
    uploading.value = false;
  }
}

// 文件操作API函数
async function renameFile(file, newName) {
  try {
    const oldPath = file.path || (currentPath.value + file.name);
    // 构建新路径
    const pathParts = oldPath.split('/');
    pathParts[pathParts.length - 1] = newName;
    const newPath = pathParts.join('/');
    
    await axios.post('/api/rename', {
      old_path: oldPath,
      new_path: newPath
    });
    notification.success('重命名成功');
    fetchFiles(currentPath.value);
  } catch (error) {
    notification.error(error.response?.data || '重命名失败');
  }
}

async function deleteFile(file, showSuccessMessage = true) {
  try {
    const filePath = file.path || (currentPath.value + file.name);
    await axios.post('/api/delete', {
      path: filePath
    });
    if (showSuccessMessage) {
      notification.success('删除成功');
      fetchFiles(currentPath.value);
    }
  } catch (error) {
    notification.error(error.response?.data || '删除失败');
    throw error; // 重新抛出错误以便批量删除时统计
  }
}

const hoverRow = ref(-1);
const hoverRowIndex = ref(-1);



function onRowMouseEnter(row, rowIndex) {
  hoverRow.value = rowIndex;
  hoverRowIndex.value = rowIndex;
}

function onRowMouseLeave() {
  hoverRow.value = -1;
  hoverRowIndex.value = -1;
}

function getRowClassName({ rowIndex }) {
  return hoverRowIndex.value === rowIndex ? 'hover-row' : '';
}

// 获取当前用户信息
async function getCurrentUser() {
  try {
    const res = await axios.get('/api/user/profile');
    if (res.status === 200 && res.data.username) {
      user.value = res.data;
      return true;
    }
  } catch (error) {
    // 如果获取用户信息失败，尝试游客登录
    try {
      const guestRes = await axios.get('/api/guest-login');
      if (guestRes.status === 200 && guestRes.data.username) {
        user.value = guestRes.data;
        return true;
      }
    } catch (guestError) {
      console.log('游客登录失败:', guestError.response?.data);
    }
  }
  return false;
}

onMounted(async () => {
  // 先加载站点信息
  loadSiteInfo();
  
  // 获取当前用户信息
  const isAuthenticated = await getCurrentUser();
  if (!isAuthenticated) {
    router.push('/login');
    return;
  }
  
  // 应用保存的主题设置
  if (isDarkMode.value) {
    document.body.classList.add('dark-mode');
  }
  
  // 如果是文件预览，不需要加载文件列表
  if (isFilePreview.value) {
    return;
  }
  
  // 获取显示路径
  let displayPath = decodeURIComponent(route.path).replace(/\\/g, '/');
  if (displayPath && !displayPath.endsWith('/')) displayPath += '/';
  if (displayPath === '/') displayPath = '/';
  
  // 将显示路径转换为实际路径
  const actualPath = displayPathToActualPath(displayPath);
  
  currentPath.value = actualPath;
  fetchFiles(actualPath);
  
  // 确保文件夹输入框正确初始化
  nextTick(() => {
    if (uploadFolderInput.value) {
      uploadFolderInput.value.setAttribute('webkitdirectory', 'true');
      uploadFolderInput.value.setAttribute('directory', 'true');
    }
  });
});

watch(() => route.path, async (newPath) => {
  // 如果未登录，尝试重新获取用户信息
  if (!user.value.username) {
    const isAuthenticated = await getCurrentUser();
    if (!isAuthenticated) {
      router.push('/login');
      return;
    }
  }
  
  // 如果是文件预览，不需要加载文件列表
  if (isFilePreview.value) {
    return;
  }
  
  // 获取显示路径
  let displayPath = decodeURIComponent(newPath).replace(/\\/g, '/');
  if (displayPath && !displayPath.endsWith('/')) displayPath += '/';
  if (displayPath === '/') displayPath = '/';
  
  // 将显示路径转换为实际路径
  const actualPath = displayPathToActualPath(displayPath);
  
  currentPath.value = actualPath;
  fetchFiles(actualPath);
});

onUnmounted(() => {
  window.removeEventListener('mousedown', closeContextMenu);
});

// 加载站点信息
async function loadSiteInfo() {
  try {
    const res = await axios.get('/api/site-info');
    siteInfo.value = res.data;
    
    console.log('MainPage 加载站点信息:', {
      background_url: siteInfo.value.background_url,
      enable_glass_effect: siteInfo.value.enable_glass_effect
    });
    
    // 应用每页显示数量
    if (siteInfo.value.items_per_page) {
      pageSize.value = parseInt(siteInfo.value.items_per_page);
    }
    
    // 应用主题色
    if (siteInfo.value.theme_color) {
      document.documentElement.style.setProperty('--theme-color', siteInfo.value.theme_color);
      // 同时设置Element Plus的主题色
      document.documentElement.style.setProperty('--el-color-primary', siteInfo.value.theme_color);
    }
    
    // 更新页面标题
    document.title = siteInfo.value.site_title;
    
    // 更新favicon
    if (siteInfo.value.favicon) {
      updateFavicon(siteInfo.value.favicon);
    }
    
    // 应用背景图片和毛玻璃效果
    applyBackgroundAndGlassEffect();
  } catch (error) {
    console.error('加载站点信息失败:', error);
  }
}

// 应用背景图片和毛玻璃效果
function applyBackgroundAndGlassEffect() {
  const body = document.body;
  
  // 应用背景图片
  if (siteInfo.value.background_url && siteInfo.value.background_url.trim()) {
    body.style.backgroundImage = `url(${siteInfo.value.background_url})`;
    body.style.backgroundSize = 'cover';
    body.style.backgroundPosition = 'center';
    body.style.backgroundRepeat = 'no-repeat';
    body.style.backgroundAttachment = 'fixed';
    body.classList.add('has-background');
    console.log('✅ MainPage 应用背景图片:', siteInfo.value.background_url);
  } else {
    body.style.backgroundImage = '';
    body.classList.remove('has-background');
    console.log('❌ MainPage 清除背景图片');
  }
  
  // 应用毛玻璃效果
  const glassElements = document.querySelectorAll('.yaolist-card, .custom-message-card, .floating-menu-card, .dialog-container, .upload-container');
  console.log('MainPage 找到元素数量:', glassElements.length);
  
  glassElements.forEach(element => {
    if (siteInfo.value.enable_glass_effect && siteInfo.value.background_url && siteInfo.value.background_url.trim()) {
      element.classList.add('glass-effect');
      console.log('✅ MainPage 应用毛玻璃效果到元素:', element.className);
    } else {
      element.classList.remove('glass-effect');
      console.log('❌ MainPage 清除毛玻璃效果:', element.className);
    }
  });
}

// 更新favicon
function updateFavicon(faviconUrl) {
  const link = document.querySelector("link[rel*='icon']") || document.createElement('link');
  link.type = 'image/x-icon';
  link.rel = 'shortcut icon';
  link.href = faviconUrl;
  document.getElementsByTagName('head')[0].appendChild(link);
}

// ===== 移动 / 复制对话框 =====
const transferDialog = ref({
  visible: false,
  sourceFile: null,
  currentPath: '/',
  action: '',
  dirs: []
});

async function openTransferDialog(file, action) {
  console.log('打开传输对话框:', { file, action });
  transferDialog.value = {
    visible: true,
    sourceFile: file,
    currentPath: '/',  // 默认为根目录
    action: action,
    dirs: []
  };
  await fetchTransferDirs('/');  // 获取根目录的内容
}

async function fetchTransferDirs(path) {
  try {
    // 规范化路径格式
    path = path.replace(/\\/g, '/');
    if (!path.startsWith('/')) path = '/' + path;
    if (!path.endsWith('/')) path += '/';
    
    const res = await axios.get('/api/files', {
      params: { path }
    });
    
    // 更新目录列表和当前路径
    transferDialog.value.dirs = res.data.filter(f => f.is_dir);
    transferDialog.value.currentPath = path.endsWith('/') ? path : path + '/';
  } catch (e) {
    notification.error('加载目录失败');
  }
}

function enterTransferDir(dir) {
  const newPath = (transferDialog.value.currentPath === '/' ? '' : transferDialog.value.currentPath) + dir.name + '/';
  fetchTransferDirs(newPath);
}

function transferGoUp() {
  if (transferDialog.value.currentPath === '/') return;
  const parts = transferDialog.value.currentPath.split('/').filter(Boolean);
  parts.pop();
  const upPath = '/' + parts.join('/') + (parts.length > 0 ? '/' : '');
  fetchTransferDirs(upPath);
}

// 添加fetchFileInfo函数
async function fetchFileInfo(path) {
  try {
    // 获取父目录路径
    const parentPath = path.substring(0, path.lastIndexOf('/'));
    const fileName = path.substring(path.lastIndexOf('/') + 1);
    
    const response = await fetch(`/api/list?path=${encodeURIComponent(parentPath)}`, {
      credentials: 'include'
    });

    if (!response.ok) {
      throw new Error('获取文件信息失败');
    }

    const files = await response.json();
    const fileInfo = files.find(file => file.name === fileName);
    
    if (!fileInfo) {
      throw new Error('找不到文件信息');
    }

    return fileInfo;
  } catch (error) {
    console.error('获取文件信息错误:', error);
    throw error;
  }
}

async function confirmTransfer() {
  if (!transferDialog.value.currentPath) {
    ElMessage.error('请选择目标路径');
    return;
  }

  const sourcePath = transferDialog.value.sourceFile.path;
  const targetPath = transferDialog.value.currentPath;
  const action = transferDialog.value.action;
  const sourceInfo = transferDialog.value.sourceFile;

  if (sourcePath === targetPath) {
    ElMessage.error('源路径与目标路径相同');
    return;
  }

  const loadingInstance = ElLoading.service({
    lock: true,
    text: `${action === 'copy' ? '复制' : '移动'}中...`,
    background: 'rgba(0, 0, 0, 0.7)'
  });

  try {
    console.log('开始传输文件:', { sourcePath, targetPath, action });
    console.log('源文件信息:', sourceInfo);

    if (sourceInfo.is_dir) {
      console.log('开始传输目录');
      // 目录操作使用transfer API
      const response = await fetch('/api/transfer', {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json'
        },
        credentials: 'include',
        body: JSON.stringify({
          src_path: sourcePath,
          dst_path: targetPath,
          action: action
        })
      });

      if (!response.ok) {
        const error = await response.text();
        throw new Error(error);
      }
    } else {
      console.log('开始传输文件');
      // 下载完整文件
      const downloadResponse = await fetch(`/api/download?path=${encodeURIComponent(sourcePath)}`);

      if (!downloadResponse.ok) {
        throw new Error('下载文件失败');
      }

      const blob = await downloadResponse.blob();
      const formData = new FormData();
      formData.append('file', blob, sourceInfo.name);
      formData.append('path', targetPath);
      formData.append('filename', sourceInfo.name);

      // 上传文件
      const uploadResponse = await fetch('/api/upload', {
        method: 'POST',
        credentials: 'include',
        body: formData
      });

      if (!uploadResponse.ok) {
        throw new Error('上传文件失败');
      }

      // 如果是移动操作，删除源文件
      if (action === 'move') {
        console.log('删除源文件');
        const deleteResponse = await fetch('/api/delete', {
          method: 'POST',
          headers: {
            'Content-Type': 'application/json'
          },
          credentials: 'include',
          body: JSON.stringify({
            path: sourcePath
          })
        });

        if (!deleteResponse.ok) {
          throw new Error('删除源文件失败');
        }
      }
    }

    loadingInstance.close();
    ElMessage.success(`${action === 'copy' ? '复制' : '移动'}成功`);
    closeTransferDialog();
    // 刷新当前目录
    fetchFiles(currentPath.value);
  } catch (error) {
    console.error('传输错误:', error);
    loadingInstance.close();
    ElMessage.error(error.message || `${action === 'copy' ? '复制' : '移动'}失败`);
  }
}

function closeTransferDialog() {
  transferDialog.value.visible = false;
  transferDialog.value = {
    visible: false,
    sourceFile: null,
    currentPath: '/',
    action: '',
    dirs: []
  };
}

// 重写菜单函数
function menuMove() {
  if (contextMenu.value.row) {
    openTransferDialog(contextMenu.value.row, 'move');
  }
  contextMenu.value.show = false;
}

function menuCopy() {
  if (contextMenu.value.row) {
    openTransferDialog(contextMenu.value.row, 'copy');
  }
  contextMenu.value.show = false;
}

function oldMenuMove() {
  if (contextMenu.value.row) {
    openTransferDialog(contextMenu.value.row, 'move');
  }
  contextMenu.value.show = false;
}

function oldMenuCopy() {
  if (contextMenu.value.row) {
    openTransferDialog(contextMenu.value.row, 'copy');
  }
  contextMenu.value.show = false;
}

// 渲染Markdown内容
function renderContent(content) {
    if (!content) return '';

    // 配置 DOMPurify
    const purifyConfig = {
      ADD_TAGS: ['script'],
      ADD_ATTR: ['type', 'src', 'id', 'class', 'style'],
      FORBID_TAGS: ['style'], // 禁用 style 标签但允许 style 属性
      FORBID_ATTR: ['onerror', 'onload', 'onunload', 'onclick', 'onmouseover', 'onmouseout'], // 禁用危险的事件处理程序
    };

    // 检查内容是否包含 HTML 标签
    const containsHtml = /<[a-z][\s\S]*>/i.test(content);

    if (containsHtml) {
      // 如果包含 HTML，直接使用 DOMPurify 清理
      return DOMPurify.sanitize(content, purifyConfig);
    } else {
      // 如果是纯文本或 Markdown，使用 marked 处理
      marked.setOptions({
        headerIds: true,
        mangle: false,
        headerPrefix: '',
        breaks: true,
        gfm: true,
        html: true
      });
      const html = marked.parse(content);
      return DOMPurify.sanitize(html, purifyConfig);
    }
  }

     const backgroundStyle = computed(() => {
     if (!siteInfo.value.background_url) return {};
     return {
       backgroundImage: `url(${siteInfo.value.background_url})`,
       backgroundSize: 'cover',
       backgroundPosition: 'center',
       backgroundAttachment: 'fixed',
       minHeight: '100vh',
       transition: 'background-image 0.3s ease'
     };
   });
</script>

<style scoped>
@import './styles/MainPage.css';

/* 右键菜单过渡动画 */
.custom-context-menu {
  transform-origin: top left;
  animation: contextMenuFadeIn 0.15s ease-out;
}

@keyframes contextMenuFadeIn {
  from {
    opacity: 0;
    transform: scale(0.95);
  }
  to {
    opacity: 1;
    transform: scale(1);
  }
}

/* 右键菜单项过渡动画 */
.context-menu-item {
  animation: menuItemSlideIn 0.2s ease-out backwards;
}

.context-menu-item:nth-child(1) { animation-delay: 0.05s; }
.context-menu-item:nth-child(2) { animation-delay: 0.1s; }
.context-menu-item:nth-child(3) { animation-delay: 0.15s; }
.context-menu-item:nth-child(4) { animation-delay: 0.2s; }
.context-menu-item:nth-child(5) { animation-delay: 0.25s; }
.context-menu-item:nth-child(6) { animation-delay: 0.3s; }

@keyframes menuItemSlideIn {
  from {
    opacity: 0;
    transform: translateX(-10px);
  }
  to {
    opacity: 1;
    transform: translateX(0);
  }
}

/* 背景图片和毛玻璃效果 */
.yaolist-flex-root {
  background-size: cover;
  background-position: center;
  background-attachment: fixed;
  min-height: 100vh;
  transition: background-image 0.3s ease;
}

.yaolist-card.glass-effect {
  background: rgba(255, 255, 255, 0.7);
  backdrop-filter: blur(10px);
  -webkit-backdrop-filter: blur(10px);
  border: 1px solid rgba(255, 255, 255, 0.7);
  box-shadow: 0 8px 32px rgba(0, 0, 0, 0.1);
}

.dark-mode .yaolist-card.glass-effect {
  background: rgba(30, 30, 30, 0.7);
  border: 1px solid rgba(255, 255, 255, 0.1);
}

.custom-message-card.glass-effect {
  background: rgba(255, 255, 255, 0.7);
  backdrop-filter: blur(10px);
  -webkit-backdrop-filter: blur(10px);
  border: 1px solid rgba(255, 255, 255, 0.7);
}

.dark-mode .custom-message-card.glass-effect {
  background: rgba(30, 30, 30, 0.7);
  border: 1px solid rgba(255, 255, 255, 0.1);
}
</style> 