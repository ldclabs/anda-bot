## 中文

1. 单一用途说明：Anda Bot 是连接 Chrome 与本机 Anda daemon 的侧边栏助手，用于把当前网页和浏览器操作上下文交给本机智能体处理。
2. `activeTab`：在用户主动发起任务时，临时获取当前标签页信息，让助手理解正在查看的网页。
3. `browsingData`：在用户请求时清理指定站点或时间范围内的缓存、本地存储等浏览数据，用于重置网页状态和排查浏览器环境问题。
4. `contextMenus`：在页面右键菜单中提供“发送此内容到 Anda”的用户触发入口，用于把用户右键选中的页面元素上下文附加到侧边栏对话。
5. `cookies`：读取、设置或删除指定网页的 Cookie，用于会话排查、登录状态辅助和网页任务执行。
6. `debugger`：通过 Chrome 调试协议执行更稳定的页面操作，例如原生点击与按键、截图、导出 PDF、读取无障碍树，以及等待网络空闲。
7. `downloads`：发起、查询、取消和打开下载任务，用于处理网页导出文件和下载结果。
8. `scripting`：在用户授权的页面中注入脚本，用于读取页面内容或执行点击、填写等浏览器操作。
9. `sidePanel`：提供 Chrome 侧边栏聊天界面，让用户在浏览网页时直接与 Anda 交互。
10. `storage`：在本地保存 Gateway URL、Bearer token、session id 和用户设置，以便自动连接本机服务。
11. `tabs`：列出、打开、切换和导航标签页，以支持跨网页研究、整理和网页任务执行。
12. `tts`：在用户需要时朗读助手回复，提供语音播放能力。
13. `webNavigation`：监听页面跳转和加载完成状态，帮助在打开、切换或导航标签页后更新页面上下文并等待页面就绪。
14. 主机权限：允许扩展在用户请求的网页上读取内容并执行浏览器动作，使本机智能体能处理不同网站上的任务。

## English

1. Single purpose: Anda Bot is a Chrome side-panel assistant that connects the browser to the local Anda daemon, sending webpage context and browser actions to the local agent.
2. `activeTab`: Temporarily accesses the active tab after a user action so the assistant can understand the page the user is viewing.
3. `browsingData`: Removes site-scoped cache, local storage, and related browsing data on user request so Anda can reset page state and troubleshoot browser workflows.
4. `contextMenus`: Adds a user-triggered "Send this content to Anda" entry to the page context menu so the extension can attach the right-clicked page element context to the side-panel conversation.
5. `cookies`: Reads, sets, and deletes cookies for user-requested pages to help with session troubleshooting, sign-in related flows, and web tasks.
6. `debugger`: Uses the Chrome DevTools protocol for more reliable page actions such as native clicks and key presses, screenshots, PDF export, accessibility tree inspection, and waiting for network idle.
7. `downloads`: Starts, lists, cancels, and opens downloads so Anda can handle files produced by web tasks.
8. `scripting`: Injects scripts into user-authorized pages to read page content or perform browser actions such as clicking and filling fields.
9. `sidePanel`: Provides the Chrome side-panel chat interface so users can talk to Anda while browsing.
10. `storage`: Stores the Gateway URL, Bearer token, session id, and user settings locally so the extension can reconnect to the local service.
11. `tabs`: Lists, opens, switches, and navigates tabs to support multi-page research, organization, and browser-based tasks.
12. `tts`: Reads assistant responses aloud when the user wants speech playback.
13. `webNavigation`: Observes navigation and page-load completion so Anda can refresh page context and wait for tabs to become ready after opening, switching, or navigating.
14. Host permissions: Allows the extension to read content and perform browser actions on user-requested webpages, so the local agent can work across different sites.

windows justification:
Anda uses the Chrome windows API only for user-requested browser automation, such as focusing the window that contains a selected tab before clicking, typing, taking screenshots, or coordinating multi-tab workflows. It does not create hidden windows or track window activity in the background.

file:///* justification:
Anda supports local-first browser workflows where users ask the assistant to inspect or automate local HTML, PDF, image, text, and other files they open in Chrome. The file:///* host permission lets the extension read page context and run requested actions on local file tabs only when Chrome users have also enabled "Allow access to file URLs" for the extension. Local file content is sent only to the connected local Anda daemon as part of a user-requested task.
