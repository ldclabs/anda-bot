import type { Locale } from './landing';

export type InfoPageKind = 'privacy' | 'terms' | 'support';

export type InfoItemCopy = {
	title: string;
	detail: string;
};

export type InfoSectionCopy = {
	title: string;
	body?: string[];
	items?: InfoItemCopy[];
};

export type InfoActionCopy = {
	label: string;
	href: string;
};

export type InfoPageCopy = {
	meta: {
		title: string;
		description: string;
	};
	eyebrow: string;
	title: string;
	intro: string;
	updated: string;
	sections: InfoSectionCopy[];
	actions?: InfoActionCopy[];
};

export type InfoLocaleCopy = {
	common: {
		home: string;
		docs: string;
		github: string;
		privacy: string;
		terms: string;
		support: string;
		languageLabel: string;
		updatedLabel: string;
		navigationLabel: string;
		moreLinks: string;
	};
	pages: Record<InfoPageKind, InfoPageCopy>;
};

const githubIssues = 'https://github.com/ldclabs/anda-bot/issues';
const githubDiscussions = 'https://github.com/ldclabs/anda-bot/discussions';
const docsUrl = 'https://docs.anda.bot';

export const infoCopy: Record<Locale, InfoLocaleCopy> = {
	en: {
		common: {
			home: 'Home',
			docs: 'Docs',
			github: 'GitHub',
			privacy: 'Privacy Policy',
			terms: 'Terms of Service',
			support: 'Support',
			languageLabel: 'Language',
			updatedLabel: 'Last updated',
			navigationLabel: 'Site navigation',
			moreLinks: 'More information'
		},
		pages: {
			privacy: {
				meta: {
					title: 'Privacy Policy - Anda Bot',
					description:
						'How the Anda Bot Chrome extension and website handle local settings, browser context, prompts, voice features, and model provider data.'
				},
				eyebrow: 'Privacy Policy',
				title: 'Privacy, local control, and browser data',
				intro:
					'Anda Bot is designed as a local-first agent bridge. The Chrome extension connects your browser to the Anda daemon you run, while the website explains and distributes the project.',
				updated: 'May 15, 2026',
				sections: [
					{
						title: 'What this policy covers',
						body: [
							'This policy covers the Anda Bot website and the Anda Bot Chrome extension. The extension is a side panel client for a local Anda daemon. The local daemon, your configured model providers, and any external services you connect may have their own data practices.',
							'The extension does not sell personal data. It exists to send your instructions and selected browser context to the local Anda runtime you configure.'
						]
					},
					{
						title: 'Data stored by the extension',
						items: [
							{
								title: 'Connection settings',
								detail:
									'The Gateway URL and Bearer token you paste into the settings panel are stored in Chrome local storage so the extension can reconnect to your local Anda daemon.'
							},
							{
								title: 'Browser session id',
								detail:
									'A stable browser session id is stored locally so Anda can keep one browser conversation thread as you switch tabs.'
							},
							{
								title: 'Conversation display state',
								detail:
									'The side panel may display conversation history returned by the local Anda daemon, but conversation records are managed by the daemon rather than by a remote extension service.'
							}
						]
					},
					{
						title: 'Browser context and permissions',
						body: [
							'When connected, the extension can register the current tab id, title, and URL with your local Anda daemon. If a task requires browser work, the daemon may ask the extension to list tabs, open or switch tabs, navigate, capture the visible tab, or run page actions such as reading content, clicking, typing, scrolling, and pressing keys.',
							'TTS permissions let Anda speak responses through Chrome when you choose voice playback. Voice capture or speech recognition only starts from user-facing voice controls and may depend on browser microphone permissions for the active page.'
						]
					},
					{
						title: 'Prompts, files, and model providers',
						body: [
							'Your prompts, selected attachments, generated transcripts, screenshots, page text, and tool results can be sent to the local Anda daemon. The daemon may then send relevant content to the model providers and services you configure in Anda.',
							'API keys and provider settings are controlled by your local Anda configuration. Review the privacy terms of each provider before sending sensitive information.'
						]
					},
					{
						title: 'Retention and control',
						items: [
							{
								title: 'Local extension data',
								detail:
									'You can clear the Gateway URL, token, and session data through Chrome extension storage or by removing the extension.'
							},
							{
								title: 'Anda runtime data',
								detail:
									'Anda stores runtime state, logs, channels, workspace files, and memory data in the local Anda home directory unless you configure a different location.'
							},
							{
								title: 'Website analytics',
								detail:
									'The current website code does not include a custom analytics tracker. Hosting infrastructure may still create standard operational logs.'
							}
						]
					},
					{
						title: 'Sensitive use',
						body: [
							'Do not send secrets, regulated records, confidential business data, or other sensitive content unless you understand where your local daemon, tools, model providers, and connected services will process it.',
							'Anda Bot is not intended for children under the age required by applicable law to use online services without parental consent.'
						]
					}
				],
				actions: [
					{ label: 'Open support', href: '/support' },
					{ label: 'GitHub issues', href: githubIssues }
				]
			},
			terms: {
				meta: {
					title: 'Terms of Service - Anda Bot',
					description:
						'Terms for using the Anda Bot website, Chrome extension, local runtime bridge, browser automation features, and open-source project resources.'
				},
				eyebrow: 'Terms of Service',
				title: 'Terms for using Anda Bot',
				intro:
					'These terms explain the responsibilities that come with using the website, Chrome extension, and local Anda runtime bridge.',
				updated: 'May 15, 2026',
				sections: [
					{
						title: 'Acceptance and scope',
						body: [
							'By using the website or Chrome extension, you agree to these terms. If you do not agree, do not use the website or extension.',
							'Anda Bot is open-source software and a local-first agent system. Some functionality depends on your local installation, model providers, browsers, operating system, and third-party tools.'
						]
					},
					{
						title: 'Your setup and accounts',
						items: [
							{
								title: 'Local daemon',
								detail:
									'You are responsible for installing, configuring, updating, and securing the local Anda Bot program and any home directory or workspace it uses.'
							},
							{
								title: 'API keys',
								detail:
									'You are responsible for your model provider accounts, API keys, usage costs, rate limits, and provider terms.'
							},
							{
								title: 'Extension token',
								detail:
									'Keep the Bearer token private. Anyone with access to it may be able to connect to your local Anda gateway while it is reachable.'
							}
						]
					},
					{
						title: 'Browser automation',
						body: [
							'The extension can help Anda interact with browser tabs and pages. You are responsible for reviewing actions before using them in accounts, administrative systems, purchases, financial workflows, production services, or other sensitive environments.',
							'Do not use Anda Bot to violate websites, terms of service, access controls, laws, or the rights of others.'
						]
					},
					{
						title: 'Content and output',
						body: [
							'You retain responsibility for the prompts, files, browser content, and other materials you provide. You are responsible for verifying generated output before relying on it.',
							'AI-generated output can be incomplete, incorrect, unsafe, or unsuitable for a specific purpose. Use professional judgment for legal, medical, financial, security, and operational decisions.'
						]
					},
					{
						title: 'Open-source license and availability',
						body: [
							'The Anda Bot source code is provided under the license in the repository. These terms do not replace the open-source license for the code.',
							'The website, extension, releases, documentation, and integrations may change, pause, or stop at any time. Features are provided as available and without a separate service-level commitment.'
						]
					},
					{
						title: 'No warranty and limitation of liability',
						body: [
							'To the maximum extent permitted by law, Anda Bot is provided as is, without warranties of any kind.',
							'To the maximum extent permitted by law, the project maintainers are not liable for indirect, incidental, special, consequential, or punitive damages, or for loss of data, profits, business, or goodwill.'
						]
					}
				],
				actions: [
					{ label: 'Read privacy policy', href: '/privacy' },
					{ label: 'GitHub repository', href: 'https://github.com/ldclabs/anda-bot' }
				]
			},
			support: {
				meta: {
					title: 'Support - Anda Bot',
					description:
						'Where to get help with Anda Bot installation, Chrome extension setup, browser token connection, local daemon issues, and bug reports.'
				},
				eyebrow: 'Support',
				title: 'Get help with Anda Bot',
				intro:
					'Anda Bot is an open-source local agent. The fastest path to support is usually to check setup, capture the exact error, and open a GitHub issue with enough detail to reproduce it.',
				updated: 'May 15, 2026',
				sections: [
					{
						title: 'Best support channels',
						items: [
							{
								title: 'Bug reports',
								detail:
									'Open a GitHub issue for reproducible extension, daemon, install, browser tool, or build problems.'
							},
							{
								title: 'Questions and ideas',
								detail:
									'Use GitHub discussions for usage questions, workflows, feature ideas, and community help.'
							},
							{
								title: 'Documentation',
								detail:
									'Use the docs and README for install commands, model provider configuration, Chrome token setup, Skills, channels, voice, and troubleshooting context.'
							}
						]
					},
					{
						title: 'Before opening an issue',
						items: [
							{
								title: 'Confirm local setup',
								detail:
									'Run anda from a terminal, confirm a model provider is configured, and make sure the daemon is reachable before testing the extension.'
							},
							{
								title: 'Regenerate the browser token',
								detail:
									'Run anda browser token --days 365 and paste both the Gateway URL and Bearer token into the side panel settings.'
							},
							{
								title: 'Check permissions',
								detail:
									'Chrome may block page injection, microphone access, file URLs, or extension actions on restricted pages. Try a normal https page when debugging.'
							}
						]
					},
					{
						title: 'Include this information',
						body: [
							'Please include your operating system, Anda Bot version, Chrome version, extension version, install method, the command you ran, the exact error message, and whether the issue happens on a fresh browser tab.',
							'Do not paste API keys, Bearer tokens, private prompts, confidential files, or sensitive screenshots into public issues.'
						]
					},
					{
						title: 'Security and privacy reports',
						body: [
							'For security-sensitive problems, avoid posting secrets or exploit details in a public issue. Open a minimal report first or use the repository security reporting options if available.',
							'If a token may have been exposed, revoke it by removing extension settings and generating a fresh browser token from the local Anda CLI.'
						]
					}
				],
				actions: [
					{ label: 'Open GitHub issues', href: githubIssues },
					{ label: 'Open discussions', href: githubDiscussions },
					{ label: 'Read docs', href: docsUrl }
				]
			}
		}
	},
	zh: {
		common: {
			home: '首页',
			docs: '文档',
			github: 'GitHub',
			privacy: '隐私政策',
			terms: '服务条款',
			support: '支持',
			languageLabel: '语言',
			updatedLabel: '最后更新',
			navigationLabel: '站点导航',
			moreLinks: '更多信息'
		},
		pages: {
			privacy: {
				meta: {
					title: '隐私政策 - Anda Bot',
					description:
						'说明 Anda Bot Chrome 扩展和网站如何处理本地设置、浏览器上下文、提示词、语音功能和模型服务商数据。'
				},
				eyebrow: '隐私政策',
				title: '隐私、本地控制与浏览器数据',
				intro:
					'Anda Bot 采用本地优先的智能体桥接方式。Chrome 扩展连接你自己运行的 Anda daemon，网站负责介绍和分发项目。',
				updated: '2026 年 5 月 15 日',
				sections: [
					{
						title: '适用范围',
						body: [
							'本政策适用于 Anda Bot 网站和 Anda Bot Chrome 扩展。扩展是本机 Anda daemon 的侧边栏客户端。本机 daemon、你配置的模型服务商，以及你连接的外部服务，可能有各自的数据处理方式。',
							'扩展不会出售个人数据。它的用途是把你的指令和你选择的浏览器上下文发送给你配置的本机 Anda 运行时。'
						]
					},
					{
						title: '扩展保存的数据',
						items: [
							{
								title: '连接设置',
								detail:
									'你粘贴到设置面板的 Gateway URL 和 Bearer token 会保存在 Chrome 本地存储中，用于重新连接本机 Anda daemon。'
							},
							{
								title: '浏览器 session id',
								detail:
									'扩展会在本地保存稳定的浏览器 session id，帮助 Anda 在你切换标签页时仍保持同一条浏览器会话线索。'
							},
							{
								title: '会话显示状态',
								detail:
									'侧边栏可能显示本机 Anda daemon 返回的会话历史，但会话记录由 daemon 管理，而不是由远程扩展服务管理。'
							}
						]
					},
					{
						title: '浏览器上下文与权限',
						body: [
							'连接后，扩展可以把当前标签页 id、标题和 URL 注册给本机 Anda daemon。如果任务需要浏览器操作，daemon 可能请求扩展列出标签页、打开或切换标签页、导航、捕获可见标签页截图，或执行读取页面、点击、输入、滚动、按键等页面动作。',
							'TTS 权限用于在你选择语音播放时通过 Chrome 朗读回答。语音录制或语音识别只会从用户可见的语音控件启动，并可能依赖当前页面的浏览器麦克风权限。'
						]
					},
					{
						title: '提示词、文件与模型服务商',
						body: [
							'你的提示词、选择的附件、生成的转写、截图、网页文本和工具结果可能会发送给本机 Anda daemon。daemon 随后可能把相关内容发送给你在 Anda 中配置的模型服务商和服务。',
							'API key 和模型服务商设置由你的本机 Anda 配置控制。发送敏感信息前，请先了解每个服务商的隐私条款。'
						]
					},
					{
						title: '保留与控制',
						items: [
							{
								title: '扩展本地数据',
								detail:
									'你可以通过 Chrome 扩展存储或卸载扩展来清除 Gateway URL、token 和 session 数据。'
							},
							{
								title: 'Anda 运行时数据',
								detail:
									'除非你配置了其他位置，Anda 会把运行时状态、日志、频道、工作区文件和记忆数据保存在本机 Anda home 目录中。'
							},
							{
								title: '网站分析',
								detail: '当前网站代码没有包含自定义分析追踪器。托管基础设施仍可能产生标准运行日志。'
							}
						]
					},
					{
						title: '敏感场景',
						body: [
							'除非你清楚本机 daemon、工具、模型服务商和连接服务会在哪里处理数据，否则不要发送密钥、受监管记录、商业机密或其他敏感内容。',
							'Anda Bot 不面向未达到适用法律要求、且未获得监护人同意即可使用在线服务的儿童。'
						]
					}
				],
				actions: [
					{ label: '打开支持页面', href: '/support' },
					{ label: 'GitHub Issues', href: githubIssues }
				]
			},
			terms: {
				meta: {
					title: '服务条款 - Anda Bot',
					description:
						'Anda Bot 网站、Chrome 扩展、本机运行时桥接、浏览器自动化功能和开源项目资源的使用条款。'
				},
				eyebrow: '服务条款',
				title: '使用 Anda Bot 的条款',
				intro: '这些条款说明使用网站、Chrome 扩展和本机 Anda 运行时桥接时需要承担的责任。',
				updated: '2026 年 5 月 15 日',
				sections: [
					{
						title: '接受与范围',
						body: [
							'使用网站或 Chrome 扩展即表示你同意这些条款。如果你不同意，请不要使用网站或扩展。',
							'Anda Bot 是开源软件和本地优先的智能体系统。部分功能依赖你的本机安装、模型服务商、浏览器、操作系统和第三方工具。'
						]
					},
					{
						title: '你的设置与账号',
						items: [
							{
								title: '本机 daemon',
								detail: '你负责安装、配置、更新和保护本机 Anda Bot 程序及其 home 目录或工作区。'
							},
							{
								title: 'API key',
								detail: '你负责自己的模型服务商账号、API key、使用费用、速率限制和服务商条款。'
							},
							{
								title: '扩展 token',
								detail:
									'请妥善保护 Bearer token。任何获得 token 的人，在你的本机 Anda gateway 可访问时都可能连接它。'
							}
						]
					},
					{
						title: '浏览器自动化',
						body: [
							'扩展可以帮助 Anda 与浏览器标签页和页面交互。在账号、管理系统、购买、金融流程、生产服务或其他敏感环境中使用前，你需要自行审查操作。',
							'不得使用 Anda Bot 违反网站规则、服务条款、访问控制、法律或他人权利。'
						]
					},
					{
						title: '内容与输出',
						body: [
							'你需要对自己提供的提示词、文件、浏览器内容和其他材料负责，也需要在依赖生成结果前自行核验。',
							'AI 输出可能不完整、不准确、不安全，或不适合特定用途。涉及法律、医疗、金融、安全和运营决策时，请使用专业判断。'
						]
					},
					{
						title: '开源许可与可用性',
						body: [
							'Anda Bot 源代码按仓库中的许可证提供。这些条款不会替代代码的开源许可证。',
							'网站、扩展、发布版本、文档和集成可能随时变化、暂停或停止。功能按现状提供，不承诺单独的服务等级。'
						]
					},
					{
						title: '无担保与责任限制',
						body: [
							'在法律允许的最大范围内，Anda Bot 按现状提供，不作任何形式的担保。',
							'在法律允许的最大范围内，项目维护者不对间接、偶然、特殊、后果性或惩罚性损害负责，也不对数据、利润、业务或商誉损失负责。'
						]
					}
				],
				actions: [
					{ label: '阅读隐私政策', href: '/privacy' },
					{ label: 'GitHub 仓库', href: 'https://github.com/ldclabs/anda-bot' }
				]
			},
			support: {
				meta: {
					title: '支持 - Anda Bot',
					description:
						'获取 Anda Bot 安装、Chrome 扩展设置、浏览器 token 连接、本机 daemon 和缺陷反馈支持。'
				},
				eyebrow: '支持',
				title: '获取 Anda Bot 帮助',
				intro:
					'Anda Bot 是开源本地智能体。最快的支持路径通常是先检查设置、记录准确错误，并在 GitHub issue 中提供足够复现信息。',
				updated: '2026 年 5 月 15 日',
				sections: [
					{
						title: '推荐支持渠道',
						items: [
							{
								title: '缺陷反馈',
								detail: '可复现的扩展、daemon、安装、浏览器工具或构建问题，请提交 GitHub issue。'
							},
							{
								title: '问题和想法',
								detail: '使用 GitHub discussions 讨论使用问题、工作流、功能想法和社区帮助。'
							},
							{
								title: '文档',
								detail:
									'文档和 README 包含安装命令、模型服务商配置、Chrome token 设置、Skills、渠道、语音和排障背景。'
							}
						]
					},
					{
						title: '提交 issue 前',
						items: [
							{
								title: '确认本机设置',
								detail: '在终端运行 anda，确认已配置模型服务商，并确保 daemon 可访问后再测试扩展。'
							},
							{
								title: '重新生成浏览器 token',
								detail:
									'运行 anda browser token --days 365，并把 Gateway URL 和 Bearer token 都粘贴到侧边栏设置中。'
							},
							{
								title: '检查权限',
								detail:
									'Chrome 可能在受限页面阻止页面注入、麦克风、file URL 或扩展动作。排障时请先尝试普通 https 页面。'
							}
						]
					},
					{
						title: '请提供的信息',
						body: [
							'请提供操作系统、Anda Bot 版本、Chrome 版本、扩展版本、安装方式、运行过的命令、完整错误信息，以及问题是否能在新的浏览器标签页复现。',
							'不要把 API key、Bearer token、私人 prompt、机密文件或敏感截图粘贴到公开 issue。'
						]
					},
					{
						title: '安全和隐私报告',
						body: [
							'安全敏感问题请避免在公开 issue 中发布密钥或利用细节。可以先提交最小报告，或使用仓库可用的安全报告渠道。',
							'如果 token 可能已泄露，请清除扩展设置，并从本机 Anda CLI 重新生成浏览器 token。'
						]
					}
				],
				actions: [
					{ label: '打开 GitHub Issues', href: githubIssues },
					{ label: '打开 Discussions', href: githubDiscussions },
					{ label: '阅读文档', href: docsUrl }
				]
			}
		}
	},
	es: {
		common: {
			home: 'Inicio',
			docs: 'Docs',
			github: 'GitHub',
			privacy: 'Privacidad',
			terms: 'Términos',
			support: 'Soporte',
			languageLabel: 'Idioma',
			updatedLabel: 'Última actualización',
			navigationLabel: 'Navegación del sitio',
			moreLinks: 'Más información'
		},
		pages: {
			privacy: {
				meta: {
					title: 'Política de privacidad - Anda Bot',
					description:
						'Cómo la extensión de Chrome y el sitio de Anda Bot manejan ajustes locales, contexto del navegador, prompts, voz y proveedores de modelos.'
				},
				eyebrow: 'Política de privacidad',
				title: 'Privacidad, control local y datos del navegador',
				intro:
					'Anda Bot funciona como un puente local: la extensión conecta Chrome con el daemon de Anda que ejecutas en tu equipo.',
				updated: '15 de mayo de 2026',
				sections: [
					{
						title: 'Alcance',
						body: [
							'Esta política cubre el sitio web y la extensión de Chrome. El daemon local, los proveedores de modelos y otros servicios conectados pueden tener sus propias prácticas.',
							'La extensión no vende datos personales; envía tus instrucciones y el contexto elegido al runtime local de Anda que configuras.'
						]
					},
					{
						title: 'Datos guardados',
						items: [
							{
								title: 'Conexión',
								detail:
									'Gateway URL y Bearer token se guardan en almacenamiento local de Chrome para reconectar con tu daemon.'
							},
							{
								title: 'Sesión del navegador',
								detail: 'Un identificador local mantiene el mismo hilo mientras cambias de pestaña.'
							},
							{
								title: 'Historial mostrado',
								detail:
									'El panel puede mostrar conversaciones devueltas por el daemon; no las gestiona un servicio remoto de la extensión.'
							}
						]
					},
					{
						title: 'Permisos del navegador',
						body: [
							'La extensión puede registrar id, título y URL de la pestaña actual. Para una tarea puede listar, abrir o cambiar pestañas, navegar, capturar pantalla visible o ejecutar acciones de página como leer, hacer clic, escribir y desplazar.',
							'TTS y voz solo se usan cuando eliges funciones de voz y pueden requerir permisos del navegador.'
						]
					},
					{
						title: 'Prompts y proveedores',
						body: [
							'Prompts, adjuntos, transcripciones, capturas, texto de páginas y resultados de herramientas pueden enviarse al daemon local y, desde allí, a los proveedores que configures.',
							'Revisa los términos de privacidad de cada proveedor antes de enviar información sensible.'
						]
					},
					{
						title: 'Control',
						body: [
							'Puedes borrar datos locales de la extensión quitando la configuración o desinstalándola. Los datos del runtime de Anda se conservan en el directorio local de Anda salvo que configures otro lugar.'
						]
					}
				],
				actions: [
					{ label: 'Soporte', href: '/support' },
					{ label: 'GitHub issues', href: githubIssues }
				]
			},
			terms: {
				meta: {
					title: 'Términos de servicio - Anda Bot',
					description:
						'Términos para usar el sitio, la extensión, el puente local y las funciones de automatización de Anda Bot.'
				},
				eyebrow: 'Términos de servicio',
				title: 'Términos para usar Anda Bot',
				intro:
					'Estos términos describen responsabilidades al usar el sitio, la extensión y el runtime local.',
				updated: '15 de mayo de 2026',
				sections: [
					{
						title: 'Aceptación',
						body: [
							'Al usar el sitio o la extensión aceptas estos términos. Si no estás de acuerdo, no los uses.',
							'Anda Bot es software open source y algunas funciones dependen de tu instalación local, proveedores, navegador y herramientas.'
						]
					},
					{
						title: 'Tu configuración',
						items: [
							{
								title: 'Daemon local',
								detail: 'Eres responsable de instalar, configurar, actualizar y proteger Anda.'
							},
							{
								title: 'API keys',
								detail: 'Eres responsable de cuentas, costos, límites y términos de proveedores.'
							},
							{ title: 'Token', detail: 'Mantén privado el Bearer token de la extensión.' }
						]
					},
					{
						title: 'Automatización',
						body: [
							'Revisa acciones antes de usarlas en cuentas, compras, sistemas administrativos, servicios de producción o flujos sensibles.',
							'No uses Anda Bot para violar leyes, controles de acceso, derechos de terceros o términos de sitios web.'
						]
					},
					{
						title: 'Contenido y salida',
						body: [
							'Eres responsable del contenido que proporcionas y de verificar la salida generada antes de confiar en ella.',
							'La salida de IA puede ser incompleta o incorrecta. Usa juicio profesional en decisiones legales, médicas, financieras, de seguridad u operación.'
						]
					},
					{
						title: 'Licencia y garantía',
						body: [
							'El código se ofrece bajo la licencia del repositorio. En la máxima medida permitida por la ley, Anda Bot se proporciona tal cual, sin garantías.'
						]
					}
				],
				actions: [
					{ label: 'Privacidad', href: '/privacy' },
					{ label: 'GitHub', href: 'https://github.com/ldclabs/anda-bot' }
				]
			},
			support: {
				meta: {
					title: 'Soporte - Anda Bot',
					description:
						'Ayuda para instalación, extensión de Chrome, token del navegador, daemon local y reportes de errores.'
				},
				eyebrow: 'Soporte',
				title: 'Obtén ayuda con Anda Bot',
				intro:
					'Para soporte, verifica la configuración local, captura el error exacto y abre un issue con pasos reproducibles.',
				updated: '15 de mayo de 2026',
				sections: [
					{
						title: 'Canales',
						items: [
							{
								title: 'Errores',
								detail:
									'Usa GitHub issues para problemas reproducibles de extensión, daemon, instalación, navegador o build.'
							},
							{
								title: 'Preguntas',
								detail: 'Usa discussions para ideas, flujos de trabajo y ayuda de comunidad.'
							},
							{
								title: 'Documentación',
								detail:
									'Consulta docs y README para instalación, modelos, token, Skills, canales y voz.'
							}
						]
					},
					{
						title: 'Antes de reportar',
						items: [
							{
								title: 'Setup local',
								detail:
									'Ejecuta anda, confirma proveedor de modelo y prueba que el daemon responde.'
							},
							{
								title: 'Token',
								detail: 'Ejecuta anda browser token --days 365 y pega Gateway URL y Bearer token.'
							},
							{
								title: 'Permisos',
								detail:
									'Prueba en una página https normal; Chrome bloquea algunas acciones en páginas restringidas.'
							}
						]
					},
					{
						title: 'Incluye',
						body: [
							'Sistema operativo, versión de Anda, Chrome y extensión, método de instalación, comando usado y error exacto.',
							'No publiques API keys, tokens, prompts privados ni capturas sensibles.'
						]
					}
				],
				actions: [
					{ label: 'GitHub issues', href: githubIssues },
					{ label: 'Discussions', href: githubDiscussions },
					{ label: 'Docs', href: docsUrl }
				]
			}
		}
	},
	fr: {
		common: {
			home: 'Accueil',
			docs: 'Docs',
			github: 'GitHub',
			privacy: 'Confidentialité',
			terms: 'Conditions',
			support: 'Support',
			languageLabel: 'Langue',
			updatedLabel: 'Dernière mise à jour',
			navigationLabel: 'Navigation du site',
			moreLinks: 'Plus d’informations'
		},
		pages: {
			privacy: {
				meta: {
					title: 'Politique de confidentialité - Anda Bot',
					description:
						'Comment l’extension Chrome et le site Anda Bot traitent les réglages locaux, le contexte du navigateur, les prompts, la voix et les fournisseurs de modèles.'
				},
				eyebrow: 'Politique de confidentialité',
				title: 'Confidentialité, contrôle local et données du navigateur',
				intro:
					'Anda Bot est un pont local entre Chrome et le daemon Anda que vous exécutez sur votre ordinateur.',
				updated: '15 mai 2026',
				sections: [
					{
						title: 'Portée',
						body: [
							'Cette politique couvre le site web et l’extension Chrome. Le daemon local, les fournisseurs de modèles et les services connectés peuvent avoir leurs propres pratiques.',
							'L’extension ne vend pas de données personnelles; elle transmet vos instructions et le contexte choisi au runtime Anda local.'
						]
					},
					{
						title: 'Données enregistrées',
						items: [
							{
								title: 'Connexion',
								detail:
									'Gateway URL et Bearer token sont stockés localement dans Chrome pour reconnecter le daemon.'
							},
							{
								title: 'Session navigateur',
								detail:
									'Un identifiant local garde le même fil de navigateur lorsque vous changez d’onglet.'
							},
							{
								title: 'Historique affiché',
								detail:
									'Le panneau peut afficher les conversations renvoyées par le daemon, sans service distant dédié à l’extension.'
							}
						]
					},
					{
						title: 'Permissions navigateur',
						body: [
							'L’extension peut enregistrer l’id, le titre et l’URL de l’onglet actif. Pour une tâche, elle peut lister, ouvrir ou changer d’onglets, naviguer, capturer l’écran visible ou exécuter des actions de page.',
							'Les fonctions TTS et voix ne sont utilisées que lorsque vous choisissez des contrôles vocaux et peuvent demander des permissions du navigateur.'
						]
					},
					{
						title: 'Prompts et fournisseurs',
						body: [
							'Prompts, pièces jointes, transcriptions, captures, texte de page et résultats d’outils peuvent être envoyés au daemon local puis aux fournisseurs configurés.',
							'Vérifiez les politiques de chaque fournisseur avant d’envoyer des informations sensibles.'
						]
					},
					{
						title: 'Contrôle',
						body: [
							'Vous pouvez supprimer les données locales de l’extension en effaçant les réglages ou en la désinstallant. Les données du runtime restent dans le dossier local d’Anda sauf configuration contraire.'
						]
					}
				],
				actions: [
					{ label: 'Support', href: '/support' },
					{ label: 'GitHub issues', href: githubIssues }
				]
			},
			terms: {
				meta: {
					title: 'Conditions d’utilisation - Anda Bot',
					description:
						'Conditions d’utilisation du site, de l’extension, du pont local et de l’automatisation navigateur.'
				},
				eyebrow: 'Conditions d’utilisation',
				title: 'Conditions pour utiliser Anda Bot',
				intro:
					'Ces conditions expliquent vos responsabilités lors de l’utilisation du site, de l’extension et du runtime local.',
				updated: '15 mai 2026',
				sections: [
					{
						title: 'Acceptation',
						body: [
							'En utilisant le site ou l’extension, vous acceptez ces conditions. Sinon, ne les utilisez pas.',
							'Anda Bot est open source et certaines fonctions dépendent de votre installation locale, de vos fournisseurs, du navigateur et des outils.'
						]
					},
					{
						title: 'Votre configuration',
						items: [
							{
								title: 'Daemon local',
								detail:
									'Vous êtes responsable de l’installation, de la configuration, des mises à jour et de la sécurité d’Anda.'
							},
							{
								title: 'Clés API',
								detail:
									'Vous êtes responsable des comptes, coûts, limites et conditions des fournisseurs.'
							},
							{ title: 'Token', detail: 'Gardez privé le Bearer token de l’extension.' }
						]
					},
					{
						title: 'Automatisation',
						body: [
							'Vérifiez les actions avant de les utiliser dans des comptes, achats, systèmes d’administration, services de production ou contextes sensibles.',
							'N’utilisez pas Anda Bot pour violer la loi, des contrôles d’accès, les droits d’autrui ou les conditions de sites web.'
						]
					},
					{
						title: 'Contenu et sortie',
						body: [
							'Vous êtes responsable du contenu fourni et de la vérification des résultats générés.',
							'La sortie IA peut être incomplète ou incorrecte. Utilisez un jugement professionnel pour les décisions sensibles.'
						]
					},
					{
						title: 'Licence et garantie',
						body: [
							'Le code est fourni sous la licence du dépôt. Dans la mesure permise par la loi, Anda Bot est fourni tel quel, sans garantie.'
						]
					}
				],
				actions: [
					{ label: 'Confidentialité', href: '/privacy' },
					{ label: 'GitHub', href: 'https://github.com/ldclabs/anda-bot' }
				]
			},
			support: {
				meta: {
					title: 'Support - Anda Bot',
					description:
						'Aide pour installation, extension Chrome, token navigateur, daemon local et rapports de bugs.'
				},
				eyebrow: 'Support',
				title: 'Obtenir de l’aide avec Anda Bot',
				intro:
					'Vérifiez la configuration locale, notez l’erreur exacte et ouvrez un issue reproductible sur GitHub.',
				updated: '15 mai 2026',
				sections: [
					{
						title: 'Canaux',
						items: [
							{
								title: 'Bugs',
								detail:
									'Utilisez GitHub issues pour les problèmes reproductibles d’extension, daemon, installation, navigateur ou build.'
							},
							{
								title: 'Questions',
								detail: 'Utilisez discussions pour les idées, workflows et aide communautaire.'
							},
							{
								title: 'Documentation',
								detail:
									'Consultez docs et README pour installation, modèles, token, Skills, canaux et voix.'
							}
						]
					},
					{
						title: 'Avant de signaler',
						items: [
							{
								title: 'Setup local',
								detail: 'Exécutez anda, vérifiez le fournisseur de modèle et testez le daemon.'
							},
							{
								title: 'Token',
								detail:
									'Exécutez anda browser token --days 365 et collez Gateway URL et Bearer token.'
							},
							{
								title: 'Permissions',
								detail:
									'Testez sur une page https normale; Chrome bloque certaines pages restreintes.'
							}
						]
					},
					{
						title: 'Inclure',
						body: [
							'Système, versions d’Anda, Chrome et extension, méthode d’installation, commande lancée et erreur exacte.',
							'Ne publiez pas de clés API, tokens, prompts privés ou captures sensibles.'
						]
					}
				],
				actions: [
					{ label: 'GitHub issues', href: githubIssues },
					{ label: 'Discussions', href: githubDiscussions },
					{ label: 'Docs', href: docsUrl }
				]
			}
		}
	},
	ru: {
		common: {
			home: 'Главная',
			docs: 'Документация',
			github: 'GitHub',
			privacy: 'Конфиденциальность',
			terms: 'Условия',
			support: 'Поддержка',
			languageLabel: 'Язык',
			updatedLabel: 'Обновлено',
			navigationLabel: 'Навигация сайта',
			moreLinks: 'Дополнительно'
		},
		pages: {
			privacy: {
				meta: {
					title: 'Политика конфиденциальности - Anda Bot',
					description:
						'Как расширение Chrome и сайт Anda Bot обрабатывают локальные настройки, контекст браузера, запросы, голос и провайдеров моделей.'
				},
				eyebrow: 'Политика конфиденциальности',
				title: 'Конфиденциальность, локальный контроль и данные браузера',
				intro:
					'Anda Bot работает как локальный мост между Chrome и daemon Anda, который запущен на вашем компьютере.',
				updated: '15 мая 2026 г.',
				sections: [
					{
						title: 'Область действия',
						body: [
							'Политика относится к сайту и расширению Chrome. Локальный daemon, провайдеры моделей и подключенные сервисы могут иметь собственные правила.',
							'Расширение не продает персональные данные; оно передает ваши инструкции и выбранный контекст в локальный runtime Anda.'
						]
					},
					{
						title: 'Сохраняемые данные',
						items: [
							{
								title: 'Подключение',
								detail:
									'Gateway URL и Bearer token хранятся локально в Chrome для повторного подключения к daemon.'
							},
							{
								title: 'Сессия браузера',
								detail: 'Локальный идентификатор помогает сохранять один поток при смене вкладок.'
							},
							{
								title: 'История',
								detail:
									'Панель может показывать разговоры, возвращенные daemon; удаленный сервис расширения ими не управляет.'
							}
						]
					},
					{
						title: 'Разрешения браузера',
						body: [
							'Расширение может передавать id, заголовок и URL активной вкладки. Для задачи оно может работать с вкладками, навигацией, видимым скриншотом и действиями на странице.',
							'TTS и голос используются только при выборе голосовых функций и могут требовать разрешения браузера.'
						]
					},
					{
						title: 'Запросы и провайдеры',
						body: [
							'Запросы, вложения, транскрипты, скриншоты, текст страниц и результаты инструментов могут отправляться локальному daemon и затем выбранным провайдерам.',
							'Проверьте политики провайдеров перед отправкой чувствительных данных.'
						]
					},
					{
						title: 'Контроль',
						body: [
							'Локальные данные расширения можно удалить через настройки Chrome или удалив расширение. Данные runtime хранятся в локальной директории Anda, если не задано другое место.'
						]
					}
				],
				actions: [
					{ label: 'Поддержка', href: '/support' },
					{ label: 'GitHub issues', href: githubIssues }
				]
			},
			terms: {
				meta: {
					title: 'Условия использования - Anda Bot',
					description:
						'Условия для сайта, расширения, локального моста и автоматизации браузера Anda Bot.'
				},
				eyebrow: 'Условия использования',
				title: 'Условия использования Anda Bot',
				intro:
					'Эти условия описывают ответственность при использовании сайта, расширения и локального runtime.',
				updated: '15 мая 2026 г.',
				sections: [
					{
						title: 'Принятие',
						body: [
							'Используя сайт или расширение, вы принимаете эти условия. Если не согласны, не используйте их.',
							'Anda Bot является open source; часть функций зависит от локальной установки, провайдеров, браузера и инструментов.'
						]
					},
					{
						title: 'Ваша настройка',
						items: [
							{
								title: 'Локальный daemon',
								detail: 'Вы отвечаете за установку, настройку, обновление и защиту Anda.'
							},
							{
								title: 'API keys',
								detail: 'Вы отвечаете за аккаунты провайдеров, расходы, лимиты и условия.'
							},
							{ title: 'Token', detail: 'Храните Bearer token расширения в тайне.' }
						]
					},
					{
						title: 'Автоматизация',
						body: [
							'Проверяйте действия перед использованием в аккаунтах, покупках, админ-системах, production-сервисах или чувствительных средах.',
							'Не используйте Anda Bot для нарушения законов, контроля доступа, прав других лиц или правил сайтов.'
						]
					},
					{
						title: 'Контент и вывод',
						body: [
							'Вы отвечаете за предоставленный контент и проверку сгенерированных результатов.',
							'AI-вывод может быть неполным или неверным. Используйте профессиональное суждение для важных решений.'
						]
					},
					{
						title: 'Лицензия и гарантии',
						body: [
							'Код предоставляется по лицензии репозитория. В пределах закона Anda Bot предоставляется как есть, без гарантий.'
						]
					}
				],
				actions: [
					{ label: 'Конфиденциальность', href: '/privacy' },
					{ label: 'GitHub', href: 'https://github.com/ldclabs/anda-bot' }
				]
			},
			support: {
				meta: {
					title: 'Поддержка - Anda Bot',
					description:
						'Помощь с установкой, расширением Chrome, browser token, локальным daemon и баг-репортами.'
				},
				eyebrow: 'Поддержка',
				title: 'Получить помощь по Anda Bot',
				intro:
					'Проверьте локальную настройку, сохраните точную ошибку и откройте issue с шагами воспроизведения.',
				updated: '15 мая 2026 г.',
				sections: [
					{
						title: 'Каналы',
						items: [
							{
								title: 'Ошибки',
								detail:
									'Используйте GitHub issues для воспроизводимых проблем расширения, daemon, установки, браузера или сборки.'
							},
							{
								title: 'Вопросы',
								detail: 'Используйте discussions для идей, workflow и помощи сообщества.'
							},
							{
								title: 'Документация',
								detail:
									'Смотрите docs и README для установки, моделей, token, Skills, каналов и голоса.'
							}
						]
					},
					{
						title: 'Перед отчетом',
						items: [
							{
								title: 'Локальная настройка',
								detail: 'Запустите anda, проверьте провайдера модели и доступность daemon.'
							},
							{
								title: 'Token',
								detail:
									'Выполните anda browser token --days 365 и вставьте Gateway URL и Bearer token.'
							},
							{
								title: 'Разрешения',
								detail:
									'Проверьте на обычной https-странице; Chrome блокирует некоторые ограниченные страницы.'
							}
						]
					},
					{
						title: 'Укажите',
						body: [
							'ОС, версии Anda, Chrome и расширения, способ установки, команду и точный текст ошибки.',
							'Не публикуйте API keys, tokens, приватные prompts или чувствительные скриншоты.'
						]
					}
				],
				actions: [
					{ label: 'GitHub issues', href: githubIssues },
					{ label: 'Discussions', href: githubDiscussions },
					{ label: 'Docs', href: docsUrl }
				]
			}
		}
	},
	ar: {
		common: {
			home: 'الرئيسية',
			docs: 'الوثائق',
			github: 'GitHub',
			privacy: 'الخصوصية',
			terms: 'الشروط',
			support: 'الدعم',
			languageLabel: 'اللغة',
			updatedLabel: 'آخر تحديث',
			navigationLabel: 'تنقل الموقع',
			moreLinks: 'معلومات إضافية'
		},
		pages: {
			privacy: {
				meta: {
					title: 'سياسة الخصوصية - Anda Bot',
					description:
						'كيف يتعامل موقع Anda Bot وإضافة Chrome مع الإعدادات المحلية وسياق المتصفح والمطالبات والصوت ومزوّدي النماذج.'
				},
				eyebrow: 'سياسة الخصوصية',
				title: 'الخصوصية والتحكم المحلي وبيانات المتصفح',
				intro: 'Anda Bot يعمل كجسر محلي بين Chrome وdaemon Anda الذي تشغله على جهازك.',
				updated: '15 مايو 2026',
				sections: [
					{
						title: 'النطاق',
						body: [
							'تنطبق هذه السياسة على الموقع وإضافة Chrome. قد يكون للdaemon المحلي ومزوّدي النماذج والخدمات المتصلة ممارسات خاصة بهم.',
							'الإضافة لا تبيع البيانات الشخصية؛ هي ترسل تعليماتك والسياق الذي تختاره إلى runtime Anda المحلي.'
						]
					},
					{
						title: 'البيانات المخزنة',
						items: [
							{
								title: 'الاتصال',
								detail:
									'يتم حفظ Gateway URL وBearer token محلياً في Chrome لإعادة الاتصال بالdaemon.'
							},
							{
								title: 'جلسة المتصفح',
								detail: 'معرّف محلي يحافظ على نفس مسار المحادثة عند تبديل التبويبات.'
							},
							{
								title: 'السجل المعروض',
								detail:
									'قد تعرض اللوحة محادثات يعيدها الdaemon، ولا تديرها خدمة بعيدة خاصة بالإضافة.'
							}
						]
					},
					{
						title: 'أذونات المتصفح',
						body: [
							'قد تسجل الإضافة id وعنوان وURL التبويب الحالي. وللمهام يمكنها إدارة التبويبات والتنقل ولقطة الشاشة المرئية وتنفيذ إجراءات الصفحة.',
							'تستخدم وظائف TTS والصوت فقط عندما تختار عناصر التحكم الصوتية وقد تتطلب أذونات المتصفح.'
						]
					},
					{
						title: 'المطالبات والمزوّدون',
						body: [
							'قد تُرسل المطالبات والمرفقات والتفريغات واللقطات ونص الصفحات ونتائج الأدوات إلى الdaemon المحلي ثم إلى المزوّدين الذين تضبطهم.',
							'راجع سياسات كل مزوّد قبل إرسال معلومات حساسة.'
						]
					},
					{
						title: 'التحكم',
						body: [
							'يمكن حذف بيانات الإضافة المحلية من إعدادات Chrome أو بإزالة الإضافة. تبقى بيانات runtime في دليل Anda المحلي ما لم تضبط موقعاً آخر.'
						]
					}
				],
				actions: [
					{ label: 'الدعم', href: '/support' },
					{ label: 'GitHub issues', href: githubIssues }
				]
			},
			terms: {
				meta: {
					title: 'شروط الخدمة - Anda Bot',
					description: 'شروط استخدام الموقع والإضافة والجسر المحلي وأتمتة المتصفح في Anda Bot.'
				},
				eyebrow: 'شروط الخدمة',
				title: 'شروط استخدام Anda Bot',
				intro: 'توضح هذه الشروط مسؤولياتك عند استخدام الموقع والإضافة وruntime المحلي.',
				updated: '15 مايو 2026',
				sections: [
					{
						title: 'القبول',
						body: [
							'باستخدام الموقع أو الإضافة فإنك توافق على هذه الشروط. إذا لم توافق، فلا تستخدمهما.',
							'Anda Bot برنامج مفتوح المصدر وبعض الوظائف تعتمد على تثبيتك المحلي والمزوّدين والمتصفح والأدوات.'
						]
					},
					{
						title: 'إعدادك',
						items: [
							{ title: 'Daemon محلي', detail: 'أنت مسؤول عن تثبيت Anda وضبطه وتحديثه وتأمينه.' },
							{
								title: 'API keys',
								detail: 'أنت مسؤول عن حسابات المزوّدين والتكاليف والحدود والشروط.'
							},
							{ title: 'Token', detail: 'حافظ على Bearer token الخاص بالإضافة بسرية.' }
						]
					},
					{
						title: 'الأتمتة',
						body: [
							'راجع الإجراءات قبل استخدامها في الحسابات أو المشتريات أو أنظمة الإدارة أو الخدمات الإنتاجية أو البيئات الحساسة.',
							'لا تستخدم Anda Bot لانتهاك القوانين أو ضوابط الوصول أو حقوق الآخرين أو شروط المواقع.'
						]
					},
					{
						title: 'المحتوى والمخرجات',
						body: [
							'أنت مسؤول عن المحتوى الذي تقدمه وعن التحقق من المخرجات قبل الاعتماد عليها.',
							'قد تكون مخرجات الذكاء الاصطناعي ناقصة أو خاطئة. استخدم حكماً مهنياً في القرارات الحساسة.'
						]
					},
					{
						title: 'الترخيص والضمان',
						body: [
							'يقدم الكود بموجب ترخيص المستودع. إلى أقصى حد يسمح به القانون، يقدم Anda Bot كما هو ودون ضمانات.'
						]
					}
				],
				actions: [
					{ label: 'الخصوصية', href: '/privacy' },
					{ label: 'GitHub', href: 'https://github.com/ldclabs/anda-bot' }
				]
			},
			support: {
				meta: {
					title: 'الدعم - Anda Bot',
					description:
						'مساعدة في التثبيت وإضافة Chrome وbrowser token والdaemon المحلي وتقارير الأخطاء.'
				},
				eyebrow: 'الدعم',
				title: 'احصل على مساعدة مع Anda Bot',
				intro: 'تحقق من الإعداد المحلي وسجل الخطأ بدقة وافتح issue بخطوات قابلة لإعادة الإنتاج.',
				updated: '15 مايو 2026',
				sections: [
					{
						title: 'القنوات',
						items: [
							{
								title: 'الأخطاء',
								detail:
									'استخدم GitHub issues للمشكلات القابلة لإعادة الإنتاج في الإضافة أو daemon أو التثبيت أو المتصفح أو البناء.'
							},
							{
								title: 'الأسئلة',
								detail: 'استخدم discussions للأفكار وسير العمل ومساعدة المجتمع.'
							},
							{
								title: 'الوثائق',
								detail: 'راجع docs وREADME للتثبيت والنماذج وtoken وSkills والقنوات والصوت.'
							}
						]
					},
					{
						title: 'قبل الإبلاغ',
						items: [
							{
								title: 'الإعداد المحلي',
								detail: 'شغل anda وتحقق من مزوّد النموذج وإمكانية الوصول إلى daemon.'
							},
							{
								title: 'Token',
								detail: 'شغل anda browser token --days 365 والصق Gateway URL وBearer token.'
							},
							{
								title: 'الأذونات',
								detail: 'جرّب صفحة https عادية؛ Chrome يحظر بعض الصفحات المقيدة.'
							}
						]
					},
					{
						title: 'أرفق',
						body: [
							'نظام التشغيل وإصدارات Anda وChrome والإضافة وطريقة التثبيت والأمر ونص الخطأ الدقيق.',
							'لا تنشر API keys أو tokens أو prompts خاصة أو لقطات حساسة.'
						]
					}
				],
				actions: [
					{ label: 'GitHub issues', href: githubIssues },
					{ label: 'Discussions', href: githubDiscussions },
					{ label: 'Docs', href: docsUrl }
				]
			}
		}
	}
};
