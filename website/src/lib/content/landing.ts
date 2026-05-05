export type Locale = 'ar' | 'zh' | 'en' | 'fr' | 'ru' | 'es';
export type TextDirection = 'ltr' | 'rtl';
export type OsKey = 'macos' | 'windows' | 'linux';

type InstallOptionCopy = {
	label: string;
	commandLabel: string;
	note: string;
	fallbackLabel?: string;
};

type ReasoningCardCopy = {
	label: string;
	title: string;
	detail: string;
};

type WorkSurfaceCopy = {
	label: string;
	detail: string;
};

type WorkCardCopy = {
	title: string;
	detail: string;
};

export type LandingCopy = {
	meta: {
		title: string;
		description: string;
		ogTitle: string;
		ogDescription: string;
	};
	nav: {
		install: string;
		reasoning: string;
		memory: string;
		surfaces: string;
	};
	language: {
		label: string;
	};
	hero: {
		badge: string;
		eyebrow: string;
		title: string;
		body: string;
		installFor: string;
		seeReasoning: string;
		proofOs: string;
		proofOsText: string;
		proofReasoning: string;
		proofReasoningText: string;
		proofMemory: string;
		proofMemoryText: string;
	};
	install: {
		eyebrow: string;
		title: string;
		detected: string;
		chooseOs: string;
		tabAria: string;
		copy: string;
		copied: string;
		copyFailed: string;
		copyAria: string;
		commandAria: string;
		alternative: string;
		steps: [string, string, string];
		requiresPrefix: string;
		requiresSuffix: string;
		options: Record<OsKey, InstallOptionCopy>;
	};
	reasoning: {
		badge: string;
		title: string;
		body: string;
		panelTitle: string;
		panelStatus: string;
		phases: [string, string, string];
		signals: Array<{ label: string; value: string; level: number }>;
		events: Array<{ time: string; phase: string; detail: string }>;
		cards: ReasoningCardCopy[];
	};
	memory: {
		badge: string;
		title: string;
		body: string;
		formationTitle: string;
		formationBody: string;
		recallTitle: string;
		recallBody: string;
		maintenanceTitle: string;
		maintenanceBody: string;
	};
	work: {
		contextRoutes: string;
		memoryRoute: string;
		badge: string;
		title: string;
		body: string;
		surfaces: WorkSurfaceCopy[];
		cards: WorkCardCopy[];
	};
	start: {
		badge: string;
		title: string;
		bodyPrefix: string;
		bodySuffix: string;
		quickStart: string;
		meetHippocampus: string;
		terminalLabel: string;
		sourceComment: string;
		goalComment: string;
		localRuntime: string;
		durableThread: string;
		inspectableBrain: string;
	};
};

export const fallbackLocale: Locale = 'en';

export const localeOrder: Locale[] = ['en', 'zh', 'es', 'fr', 'ru', 'ar'];

export const localeMeta: Record<
	Locale,
	{ label: string; nativeName: string; htmlLang: string; dir: TextDirection }
> = {
	ar: { label: 'Arabic', nativeName: 'العربية', htmlLang: 'ar', dir: 'rtl' },
	zh: { label: 'Chinese', nativeName: '中文', htmlLang: 'zh-CN', dir: 'ltr' },
	en: { label: 'English', nativeName: 'English', htmlLang: 'en', dir: 'ltr' },
	fr: { label: 'French', nativeName: 'Français', htmlLang: 'fr', dir: 'ltr' },
	ru: { label: 'Russian', nativeName: 'Русский', htmlLang: 'ru', dir: 'ltr' },
	es: { label: 'Spanish', nativeName: 'Español', htmlLang: 'es', dir: 'ltr' }
};

export const landingCopy: Record<Locale, LandingCopy> = {
	en: {
		meta: {
			title: 'Install Anda Bot - Long-horizon memory agent',
			description:
				'Install Anda Bot, a local AI agent with long-horizon reasoning, continuous learning, and a graph memory brain powered by Anda Hippocampus.',
			ogTitle: 'Install Anda Bot - Long-horizon memory agent',
			ogDescription:
				'Run Anda locally and give it goals that can continue across compacted conversations, memory recall, tools, files, and channels.'
		},
		nav: {
			install: 'Install',
			reasoning: 'Reasoning',
			memory: 'Memory',
			surfaces: 'Surfaces'
		},
		language: { label: 'Language' },
		hero: {
			badge: 'Installable local AI agent',
			eyebrow: 'continuous learning · long-horizon reasoning',
			title: 'Install Anda. Give it a goal that can outlive the chat window.',
			body: 'I am the local agent that keeps learning while we work. My Hippocampus brain remembers what matters, and my goal loop can compact context, open the next linked conversation, and keep reasoning until the objective is complete.',
			installFor: 'Install for {os}',
			seeReasoning: 'See long reasoning',
			proofOs: 'OS',
			proofOsText: 'auto-detect',
			proofReasoning: 'hours+',
			proofReasoningText: 'reasoning',
			proofMemory: 'graph',
			proofMemoryText: 'memory'
		},
		install: {
			eyebrow: 'Install latest release',
			title: 'Run Anda locally',
			detected: 'detected {os}',
			chooseOs: 'choose OS',
			tabAria: 'Install method by operating system',
			copy: 'Copy',
			copied: 'Copied',
			copyFailed: 'Copy failed',
			copyAria: 'Copy install command',
			commandAria: 'Copy the install command',
			alternative: 'Alternative: {method}',
			steps: ['Install release', 'Add or export key', 'Run'],
			requiresPrefix: 'Requires at least one model provider API key in config or env. Anda creates',
			requiresSuffix: 'on first launch.',
			options: {
				macos: {
					label: 'macOS',
					commandLabel: 'Shell script',
					note: 'The install script fetches the latest release and curated skills for macOS.',
					fallbackLabel: 'Homebrew'
				},
				windows: {
					label: 'Windows',
					commandLabel: 'PowerShell',
					note: 'Run this in PowerShell, then open a new terminal and start Anda.'
				},
				linux: {
					label: 'Linux',
					commandLabel: 'Shell script',
					note: 'The install script fetches the latest release for your local runtime.'
				}
			}
		},
		reasoning: {
			badge: 'Long-horizon reasoning',
			title: 'A goal can keep moving long after a normal chat would run out of road.',
			body: 'Anda can keep a session alive across many linked conversations. When context grows, it compacts the current state, preserves the objective, and continues reasoning with almost no practical limit on duration or turns.',
			panelTitle: 'Anda session loop',
			panelStatus: 'goal://active',
			phases: ['reason', 'compact', 'continue'],
			signals: [
				{ label: 'objective', value: 'active', level: 92 },
				{ label: 'context', value: 'compacted', level: 84 },
				{ label: 'memory', value: 'forming', level: 76 },
				{ label: 'tools', value: 'ready', level: 68 }
			],
			events: [
				{ time: '00:01', phase: 'goal', detail: 'objective accepted and session opened' },
				{
					time: '47:18',
					phase: 'compact',
					detail: 'context summarized into the next conversation'
				},
				{ time: 'hours+', phase: 'continue', detail: 'reasoning resumes until the target is done' }
			],
			cards: [
				{
					label: 'Goal loop',
					title: 'Keeps working after one answer',
					detail:
						'Give Anda an objective and it can check progress, continue, and wait for the next useful action instead of stopping at a fixed turn count.'
				},
				{
					label: 'Context handoff',
					title: 'Compacts and carries the thread forward',
					detail:
						'When a conversation gets large, Anda summarizes the live state, opens the next linked conversation, and continues the same session.'
				},
				{
					label: 'Memory formation',
					title: 'Learns from successful work',
					detail:
						'Useful turns flow into Hippocampus, where relationships, decisions, preferences, and timelines become recallable working context.'
				}
			]
		},
		memory: {
			badge: 'Anda Hippocampus',
			title: 'Memory is how the work gets better every time.',
			body: 'Hippocampus turns useful conversations into a living Cognitive Nexus: people, projects, decisions, preferences, events, and the relationships between them.',
			formationTitle: 'Formation',
			formationBody:
				'I turn successful turns, files, channels, and tool results into memories with source, time, and meaning.',
			recallTitle: 'Recall',
			recallBody:
				'When a goal needs history, I retrieve context that explains the present task instead of matching nearest text alone.',
			maintenanceTitle: 'Maintenance',
			maintenanceBody:
				'I consolidate, merge, decay, and preserve timelines so durable knowledge stays reachable while old noise fades.'
		},
		work: {
			contextRoutes: 'context routes',
			memoryRoute: 'anda://memory',
			badge: 'Where I work',
			title: 'Bring long memory to the places your context is born.',
			body: 'I can live in the terminal, join your chat channels, listen and speak by voice, read files, run tools, and keep scheduled work moving while the same memory thread follows.',
			surfaces: [
				{
					label: 'goals',
					detail: 'Long tasks stay attached to a session until the outcome is real.'
				},
				{ label: 'voice', detail: 'Spoken context can become part of the same working memory.' },
				{
					label: 'shell',
					detail: 'Local commands and files give memory something concrete to reason over.'
				},
				{
					label: 'channels',
					detail: 'Context born in teams can follow the work instead of vanishing.'
				}
			],
			cards: [
				{
					title: 'Local-first shell',
					detail:
						'A grounded workspace for commands, files, experiments, and multi-step automation.'
				},
				{
					title: 'Multi-channel chat',
					detail: 'Conversations around the work can become part of the working thread.'
				},
				{
					title: 'Your keys and models',
					detail: 'Connect the provider you trust and keep the runtime close to your machine.'
				},
				{
					title: 'Inspectable memory',
					detail: 'Open-source components keep the shape of my brain visible and auditable.'
				}
			]
		},
		start: {
			badge: 'After install',
			title: 'Run Anda from any terminal with a model key.',
			bodyPrefix: 'Use an environment key for the first run, or save provider keys in',
			bodySuffix: 'for future sessions.',
			quickStart: 'Quick start',
			meetHippocampus: 'Meet Hippocampus',
			terminalLabel: 'run command',
			sourceComment: 'Start now with an environment key:',
			goalComment: 'Or save api_key in config.yaml, then run:',
			localRuntime: 'local runtime',
			durableThread: 'durable thread',
			inspectableBrain: 'inspectable brain'
		}
	},
	zh: {
		meta: {
			title: '安装 Anda Bot - 具备长程推理的记忆 Agent',
			description:
				'安装 Anda Bot，一个具备长程推理、持续学习和 Anda Hippocampus 图记忆大脑的本地 AI Agent。',
			ogTitle: '安装 Anda Bot - 具备长程推理的记忆 Agent',
			ogDescription: '在本地运行 Anda，让目标跨越压缩后的对话、记忆召回、工具、文件和频道持续推进。'
		},
		nav: { install: '安装', reasoning: '推理', memory: '记忆', surfaces: '场景' },
		language: { label: '语言' },
		hero: {
			badge: '可本地安装的 AI Agent',
			eyebrow: '持续学习 · 长程推理',
			title: '安装 Anda。给它一个能跨越聊天窗口的目标。',
			body: '我是会在工作中持续学习的本地 Agent。Hippocampus 大脑会记住真正重要的上下文，而目标循环可以压缩上下文、开启下一个关联对话，并一直推理直到目标完成。',
			installFor: '安装 {os} 版本',
			seeReasoning: '查看长程推理',
			proofOs: 'OS',
			proofOsText: '自动识别',
			proofReasoning: '数小时+',
			proofReasoningText: '持续推理',
			proofMemory: '图谱',
			proofMemoryText: '记忆'
		},
		install: {
			eyebrow: '安装最新版本',
			title: '在本地运行 Anda',
			detected: '已识别 {os}',
			chooseOs: '选择系统',
			tabAria: '按操作系统选择安装方式',
			copy: '复制',
			copied: '已复制',
			copyFailed: '复制失败',
			copyAria: '复制安装命令',
			commandAria: '点击复制安装命令',
			alternative: '备选：{method}',
			steps: ['安装发布版', '配置或导出密钥', '运行'],
			requiresPrefix: '至少需要一个配置文件或环境变量中的模型服务商 API key。Anda 首次启动时会创建',
			requiresSuffix: '配置文件。',
			options: {
				macos: {
					label: 'macOS',
					commandLabel: 'Shell 脚本',
					note: '安装脚本会为 macOS 获取最新发布版和精选 skills。',
					fallbackLabel: 'Homebrew'
				},
				windows: {
					label: 'Windows',
					commandLabel: 'PowerShell',
					note: '在 PowerShell 中运行该命令，然后打开新的终端启动 Anda。'
				},
				linux: {
					label: 'Linux',
					commandLabel: 'Shell 脚本',
					note: '安装脚本会为你的本地运行环境获取最新发布版。'
				}
			}
		},
		reasoning: {
			badge: '长程推理',
			title: '一个目标可以在普通聊天耗尽之后继续向前推进。',
			body: 'Anda 可以让同一个 session 跨越多个关联 conversation 保持活跃。当上下文变大时，它会压缩当前状态、保留目标，并继续推理，在实际使用中几乎不受时长或轮次限制。',
			panelTitle: 'Anda session 循环',
			panelStatus: 'goal://active',
			phases: ['推理', '压缩', '继续'],
			signals: [
				{ label: '目标', value: '进行中', level: 92 },
				{ label: '上下文', value: '已压缩', level: 84 },
				{ label: '记忆', value: '形成中', level: 76 },
				{ label: '工具', value: '就绪', level: 68 }
			],
			events: [
				{ time: '00:01', phase: '目标', detail: '目标已接收，session 已开启' },
				{ time: '47:18', phase: '压缩', detail: '任务状态总结到下一个 conversation' },
				{ time: '数小时+', phase: '继续', detail: '持续推理直到目标完成' }
			],
			cards: [
				{
					label: '目标循环',
					title: '不会在一次回答后停下',
					detail:
						'给 Anda 一个目标，它可以检查进展、继续推进，并等待下一步有用动作，而不是被固定轮次截断。'
				},
				{
					label: '上下文交接',
					title: '压缩并携带任务线索继续',
					detail:
						'当 conversation 变大时，Anda 会总结当前状态，开启下一个关联 conversation，并延续同一个 session。'
				},
				{
					label: '记忆形成',
					title: '从成功工作中持续学习',
					detail:
						'有价值的轮次会进入 Hippocampus，让关系、决策、偏好和时间线成为之后可召回的工作上下文。'
				}
			]
		},
		memory: {
			badge: 'Anda Hippocampus',
			title: '记忆让每一次工作都变得更好。',
			body: 'Hippocampus 会把有用的对话转化为活的 Cognitive Nexus：人物、项目、决策、偏好、事件，以及它们之间的关系。',
			formationTitle: '形成',
			formationBody: '我会把成功的对话轮次、文件、频道和工具结果转化为带来源、时间和意义的记忆。',
			recallTitle: '召回',
			recallBody: '当目标需要历史上下文时，我会召回能解释当前任务的内容，而不仅是匹配相近文本。',
			maintenanceTitle: '维护',
			maintenanceBody: '我会合并、巩固、衰减并保留时间线，让重要知识始终可达，旧噪声自然淡出。'
		},
		work: {
			contextRoutes: '上下文路径',
			memoryRoute: 'anda://memory',
			badge: '工作场景',
			title: '把长记忆带到上下文诞生的地方。',
			body: '我可以在终端里工作，加入聊天频道，听你说也能开口回应，读取文件，运行工具，并在同一条记忆线索中推进定时任务。',
			surfaces: [
				{ label: '目标', detail: '长任务会保持在同一个 session 中，直到结果真正落地。' },
				{ label: '语音', detail: '语音上下文也可以成为同一份工作记忆的一部分。' },
				{ label: '终端', detail: '本地命令和文件让记忆拥有可以推理的真实材料。' },
				{ label: '频道', detail: '团队沟通中产生的上下文不会随着聊天窗口消失。' }
			],
			cards: [
				{ title: '本地优先终端', detail: '用于命令、文件、实验和多步骤自动化的扎实工作区。' },
				{ title: '多频道聊天', detail: '围绕工作的对话可以进入同一条持续工作线索。' },
				{ title: '你的密钥和模型', detail: '连接你信任的模型服务商，并让运行时靠近你的机器。' },
				{ title: '可检查的记忆', detail: '开源组件让我的大脑结构保持可见、可审计。' }
			]
		},
		start: {
			badge: '安装之后',
			title: '带上模型密钥，就可以在任意终端运行 Anda。',
			bodyPrefix: '第一次运行可以临时使用环境变量，也可以把 provider 密钥保存到',
			bodySuffix: '供后续会话使用。',
			quickStart: '快速开始',
			meetHippocampus: '了解 Hippocampus',
			terminalLabel: '运行命令',
			sourceComment: '用环境变量立即启动：',
			goalComment: '或把 api_key 保存到 config.yaml 后运行：',
			localRuntime: '本地运行时',
			durableThread: '持久线索',
			inspectableBrain: '可检查大脑'
		}
	},
	es: {
		meta: {
			title: 'Instala Anda Bot - Agente de memoria para razonamiento largo',
			description:
				'Instala Anda Bot, un agente local de IA con razonamiento de largo horizonte, aprendizaje continuo y una memoria en grafo impulsada por Anda Hippocampus.',
			ogTitle: 'Instala Anda Bot - Agente de memoria para razonamiento largo',
			ogDescription:
				'Ejecuta Anda localmente y dale objetivos que continúan entre conversaciones compactadas, memoria, herramientas, archivos y canales.'
		},
		nav: {
			install: 'Instalar',
			reasoning: 'Razonamiento',
			memory: 'Memoria',
			surfaces: 'Entornos'
		},
		language: { label: 'Idioma' },
		hero: {
			badge: 'Agente local de IA instalable',
			eyebrow: 'aprendizaje continuo · razonamiento de largo horizonte',
			title: 'Instala Anda. Dale un objetivo que sobreviva a la ventana del chat.',
			body: 'Soy el agente local que sigue aprendiendo mientras trabajamos. Mi cerebro Hippocampus recuerda lo importante, y mi bucle de objetivos puede compactar contexto, abrir la siguiente conversación enlazada y seguir razonando hasta completar el objetivo.',
			installFor: 'Instalar para {os}',
			seeReasoning: 'Ver razonamiento largo',
			proofOs: 'SO',
			proofOsText: 'detección automática',
			proofReasoning: 'horas+',
			proofReasoningText: 'razonamiento',
			proofMemory: 'grafo',
			proofMemoryText: 'memoria'
		},
		install: {
			eyebrow: 'Instala la última versión',
			title: 'Ejecuta Anda localmente',
			detected: '{os} detectado',
			chooseOs: 'elige SO',
			tabAria: 'Método de instalación por sistema operativo',
			copy: 'Copiar',
			copied: 'Copiado',
			copyFailed: 'No se pudo copiar',
			copyAria: 'Copiar comando de instalación',
			commandAria: 'Copiar el comando de instalación',
			alternative: 'Alternativa: {method}',
			steps: ['Instala la versión', 'Añade o exporta la clave', 'Ejecuta'],
			requiresPrefix:
				'Requiere al menos una clave API de proveedor de modelos en config o env. Anda crea',
			requiresSuffix: 'en el primer inicio.',
			options: {
				macos: {
					label: 'macOS',
					commandLabel: 'Script de shell',
					note: 'El script instala la última versión y las skills seleccionadas para macOS.',
					fallbackLabel: 'Homebrew'
				},
				windows: {
					label: 'Windows',
					commandLabel: 'PowerShell',
					note: 'Ejecuta esto en PowerShell, luego abre una terminal nueva e inicia Anda.'
				},
				linux: {
					label: 'Linux',
					commandLabel: 'Script de shell',
					note: 'El script de instalación descarga la última versión para tu entorno local.'
				}
			}
		},
		reasoning: {
			badge: 'Razonamiento de largo horizonte',
			title:
				'Un objetivo puede seguir avanzando mucho después de que un chat normal se quedaría sin camino.',
			body: 'Anda puede mantener viva una sesión a través de muchas conversaciones enlazadas. Cuando crece el contexto, compacta el estado actual, conserva el objetivo y continúa razonando casi sin límite práctico de duración o turnos.',
			panelTitle: 'Bucle de sesión de Anda',
			panelStatus: 'goal://active',
			phases: ['razonar', 'compactar', 'continuar'],
			signals: [
				{ label: 'objetivo', value: 'activo', level: 92 },
				{ label: 'contexto', value: 'compactado', level: 84 },
				{ label: 'memoria', value: 'formándose', level: 76 },
				{ label: 'herramientas', value: 'listas', level: 68 }
			],
			events: [
				{ time: '00:01', phase: 'objetivo', detail: 'objetivo aceptado y sesión abierta' },
				{
					time: '47:18',
					phase: 'compactar',
					detail: 'contexto resumido en la siguiente conversación'
				},
				{ time: 'horas+', phase: 'continuar', detail: 'el razonamiento se reanuda hasta terminar' }
			],
			cards: [
				{
					label: 'Bucle de objetivo',
					title: 'Sigue trabajando después de una respuesta',
					detail:
						'Dale a Anda un objetivo y puede revisar progreso, continuar y esperar la siguiente acción útil en lugar de detenerse en un número fijo de turnos.'
				},
				{
					label: 'Traspaso de contexto',
					title: 'Compacta y lleva el hilo hacia adelante',
					detail:
						'Cuando una conversación crece, Anda resume el estado vivo, abre la siguiente conversación enlazada y continúa la misma sesión.'
				},
				{
					label: 'Formación de memoria',
					title: 'Aprende del trabajo que sale bien',
					detail:
						'Los turnos útiles fluyen a Hippocampus, donde relaciones, decisiones, preferencias y líneas de tiempo se vuelven contexto recuperable.'
				}
			]
		},
		memory: {
			badge: 'Anda Hippocampus',
			title: 'La memoria hace que el trabajo mejore cada vez.',
			body: 'Hippocampus convierte conversaciones útiles en un Cognitive Nexus vivo: personas, proyectos, decisiones, preferencias, eventos y las relaciones entre ellos.',
			formationTitle: 'Formación',
			formationBody:
				'Convierto turnos exitosos, archivos, canales y resultados de herramientas en recuerdos con fuente, tiempo y significado.',
			recallTitle: 'Recuerdo',
			recallBody:
				'Cuando un objetivo necesita historia, recupero el contexto que explica la tarea actual en lugar de solo buscar texto cercano.',
			maintenanceTitle: 'Mantenimiento',
			maintenanceBody:
				'Consolido, fusiono, reduzco y preservo líneas de tiempo para que el conocimiento durable siga accesible mientras el ruido viejo se desvanece.'
		},
		work: {
			contextRoutes: 'rutas de contexto',
			memoryRoute: 'anda://memory',
			badge: 'Dónde trabajo',
			title: 'Lleva memoria larga a los lugares donde nace tu contexto.',
			body: 'Puedo vivir en la terminal, unirme a tus canales de chat, escuchar y hablar por voz, leer archivos, ejecutar herramientas y mantener trabajo programado mientras sigue el mismo hilo de memoria.',
			surfaces: [
				{
					label: 'objetivos',
					detail: 'Las tareas largas permanecen unidas a una sesión hasta que el resultado es real.'
				},
				{
					label: 'voz',
					detail: 'El contexto hablado puede convertirse en parte de la misma memoria de trabajo.'
				},
				{
					label: 'shell',
					detail: 'Los comandos y archivos locales dan a la memoria material concreto para razonar.'
				},
				{
					label: 'canales',
					detail: 'El contexto que nace en equipos puede seguir al trabajo en vez de desaparecer.'
				}
			],
			cards: [
				{
					title: 'Shell local primero',
					detail:
						'Un espacio de trabajo concreto para comandos, archivos, experimentos y automatización de varios pasos.'
				},
				{
					title: 'Chat multicanal',
					detail: 'Las conversaciones alrededor del trabajo pueden formar parte del hilo activo.'
				},
				{
					title: 'Tus claves y modelos',
					detail: 'Conecta el proveedor en el que confías y mantén el runtime cerca de tu máquina.'
				},
				{
					title: 'Memoria inspeccionable',
					detail:
						'Los componentes open source mantienen visible y auditable la forma de mi cerebro.'
				}
			]
		},
		start: {
			badge: 'Después de instalar',
			title: 'Ejecuta Anda desde cualquier terminal con una clave de modelo.',
			bodyPrefix: 'Usa una clave de entorno en el primer arranque, o guarda claves en',
			bodySuffix: 'para sesiones futuras.',
			quickStart: 'Inicio rápido',
			meetHippocampus: 'Conocer Hippocampus',
			terminalLabel: 'comando de ejecución',
			sourceComment: 'Inicia ahora con una clave de entorno:',
			goalComment: 'O guarda api_key en config.yaml y ejecuta:',
			localRuntime: 'runtime local',
			durableThread: 'hilo durable',
			inspectableBrain: 'cerebro inspeccionable'
		}
	},
	fr: {
		meta: {
			title: 'Installer Anda Bot - Agent mémoire pour raisonnement long',
			description:
				'Installez Anda Bot, un agent IA local avec raisonnement de longue durée, apprentissage continu et mémoire en graphe propulsée par Anda Hippocampus.',
			ogTitle: 'Installer Anda Bot - Agent mémoire pour raisonnement long',
			ogDescription:
				'Exécutez Anda localement et confiez-lui des objectifs qui continuent à travers conversations compactées, mémoire, outils, fichiers et canaux.'
		},
		nav: {
			install: 'Installer',
			reasoning: 'Raisonnement',
			memory: 'Mémoire',
			surfaces: 'Espaces'
		},
		language: { label: 'Langue' },
		hero: {
			badge: 'Agent IA local installable',
			eyebrow: 'apprentissage continu · raisonnement de longue durée',
			title: 'Installez Anda. Donnez-lui un objectif qui survit à la fenêtre de chat.',
			body: 'Je suis l’agent local qui continue d’apprendre pendant que nous travaillons. Mon cerveau Hippocampus retient ce qui compte, et ma boucle d’objectifs peut compacter le contexte, ouvrir la conversation liée suivante et raisonner jusqu’à ce que l’objectif soit atteint.',
			installFor: 'Installer pour {os}',
			seeReasoning: 'Voir le raisonnement long',
			proofOs: 'OS',
			proofOsText: 'détection auto',
			proofReasoning: 'heures+',
			proofReasoningText: 'raisonnement',
			proofMemory: 'graphe',
			proofMemoryText: 'mémoire'
		},
		install: {
			eyebrow: 'Installer la dernière version',
			title: 'Exécuter Anda localement',
			detected: '{os} détecté',
			chooseOs: 'choisir OS',
			tabAria: 'Méthode d’installation par système d’exploitation',
			copy: 'Copier',
			copied: 'Copié',
			copyFailed: 'Copie échouée',
			copyAria: 'Copier la commande d’installation',
			commandAria: 'Copier la commande d’installation',
			alternative: 'Alternative : {method}',
			steps: ['Installer la version', 'Ajouter ou exporter la clé', 'Lancer'],
			requiresPrefix:
				'Nécessite au moins une clé API de fournisseur de modèle en config ou env. Anda crée',
			requiresSuffix: 'au premier lancement.',
			options: {
				macos: {
					label: 'macOS',
					commandLabel: 'Script shell',
					note: 'Le script installe la dernière version et les skills sélectionnés pour macOS.',
					fallbackLabel: 'Homebrew'
				},
				windows: {
					label: 'Windows',
					commandLabel: 'PowerShell',
					note: 'Exécutez cette commande dans PowerShell, puis ouvrez un nouveau terminal et lancez Anda.'
				},
				linux: {
					label: 'Linux',
					commandLabel: 'Script shell',
					note: 'Le script d’installation récupère la dernière version pour votre runtime local.'
				}
			}
		},
		reasoning: {
			badge: 'Raisonnement de longue durée',
			title: 'Un objectif peut continuer bien après qu’un chat ordinaire aurait épuisé sa route.',
			body: 'Anda peut garder une session active à travers de nombreuses conversations liées. Quand le contexte grossit, il compacte l’état actuel, préserve l’objectif et continue à raisonner avec presque aucune limite pratique de durée ou de tours.',
			panelTitle: 'Boucle de session Anda',
			panelStatus: 'goal://active',
			phases: ['raisonner', 'compacter', 'continuer'],
			signals: [
				{ label: 'objectif', value: 'actif', level: 92 },
				{ label: 'contexte', value: 'compacté', level: 84 },
				{ label: 'mémoire', value: 'en formation', level: 76 },
				{ label: 'outils', value: 'prêts', level: 68 }
			],
			events: [
				{ time: '00:01', phase: 'objectif', detail: 'objectif accepté et session ouverte' },
				{
					time: '47:18',
					phase: 'compact',
					detail: 'contexte résumé dans la conversation suivante'
				},
				{ time: 'heures+', phase: 'continuer', detail: 'le raisonnement reprend jusqu’à la fin' }
			],
			cards: [
				{
					label: 'Boucle d’objectif',
					title: 'Continue après une réponse',
					detail:
						'Donnez un objectif à Anda : il peut vérifier la progression, continuer et attendre l’action utile suivante au lieu de s’arrêter à un nombre fixe de tours.'
				},
				{
					label: 'Passage de contexte',
					title: 'Compacte et porte le fil plus loin',
					detail:
						'Quand une conversation devient grande, Anda résume l’état vivant, ouvre la conversation liée suivante et poursuit la même session.'
				},
				{
					label: 'Formation de mémoire',
					title: 'Apprend du travail réussi',
					detail:
						'Les tours utiles vont vers Hippocampus, où relations, décisions, préférences et chronologies deviennent du contexte rappelable.'
				}
			]
		},
		memory: {
			badge: 'Anda Hippocampus',
			title: 'La mémoire rend le travail meilleur à chaque fois.',
			body: 'Hippocampus transforme les conversations utiles en Cognitive Nexus vivant : personnes, projets, décisions, préférences, événements et relations entre eux.',
			formationTitle: 'Formation',
			formationBody:
				'Je transforme les tours réussis, fichiers, canaux et résultats d’outils en souvenirs avec source, temps et sens.',
			recallTitle: 'Rappel',
			recallBody:
				'Quand un objectif a besoin d’historique, je récupère le contexte qui explique la tâche actuelle plutôt que de seulement chercher le texte le plus proche.',
			maintenanceTitle: 'Maintenance',
			maintenanceBody:
				'Je consolide, fusionne, fais décroître et préserve les chronologies afin que le savoir durable reste accessible pendant que le bruit ancien s’efface.'
		},
		work: {
			contextRoutes: 'routes de contexte',
			memoryRoute: 'anda://memory',
			badge: 'Où je travaille',
			title: 'Apportez une mémoire longue là où votre contexte naît.',
			body: 'Je peux vivre dans le terminal, rejoindre vos canaux de chat, écouter et parler en vocal, lire des fichiers, lancer des outils et faire avancer le travail planifié pendant que le même fil mémoire suit.',
			surfaces: [
				{
					label: 'objectifs',
					detail:
						'Les tâches longues restent attachées à une session jusqu’à ce que le résultat soit réel.'
				},
				{ label: 'voix', detail: 'Le contexte parlé peut rejoindre la même mémoire de travail.' },
				{
					label: 'shell',
					detail:
						'Les commandes et fichiers locaux donnent à la mémoire une matière concrète à raisonner.'
				},
				{
					label: 'canaux',
					detail: 'Le contexte né dans les équipes peut suivre le travail au lieu de disparaître.'
				}
			],
			cards: [
				{
					title: 'Shell local-first',
					detail:
						'Un espace de travail ancré pour commandes, fichiers, expériences et automatisation multi-étapes.'
				},
				{
					title: 'Chat multicanal',
					detail: 'Les conversations autour du travail peuvent rejoindre le fil actif.'
				},
				{
					title: 'Vos clés et modèles',
					detail:
						'Connectez le fournisseur de confiance et gardez le runtime près de votre machine.'
				},
				{
					title: 'Mémoire inspectable',
					detail: 'Les composants open source rendent la forme de mon cerveau visible et auditable.'
				}
			]
		},
		start: {
			badge: 'Après installation',
			title: 'Lancez Anda depuis n’importe quel terminal avec une clé modèle.',
			bodyPrefix:
				'Utilisez une clé en variable d’environnement au premier lancement, ou gardez les clés dans',
			bodySuffix: 'pour les sessions suivantes.',
			quickStart: 'Démarrage rapide',
			meetHippocampus: 'Découvrir Hippocampus',
			terminalLabel: 'commande',
			sourceComment: 'Démarrer avec une clé d’environnement :',
			goalComment: 'Ou enregistrer api_key dans config.yaml, puis lancer :',
			localRuntime: 'runtime local',
			durableThread: 'fil durable',
			inspectableBrain: 'cerveau inspectable'
		}
	},
	ru: {
		meta: {
			title: 'Установите Anda Bot - агент памяти для долгого рассуждения',
			description:
				'Установите Anda Bot, локального ИИ-агента с долгим рассуждением, непрерывным обучением и графовой памятью на базе Anda Hippocampus.',
			ogTitle: 'Установите Anda Bot - агент памяти для долгого рассуждения',
			ogDescription:
				'Запускайте Anda локально и задавайте цели, которые продолжаются через сжатые разговоры, память, инструменты, файлы и каналы.'
		},
		nav: { install: 'Установка', reasoning: 'Рассуждение', memory: 'Память', surfaces: 'Среды' },
		language: { label: 'Язык' },
		hero: {
			badge: 'Локальный ИИ-агент для установки',
			eyebrow: 'непрерывное обучение · долгий горизонт рассуждения',
			title: 'Установите Anda. Дайте цель, которая переживет окно чата.',
			body: 'Я локальный агент, который продолжает учиться во время работы. Мозг Hippocampus запоминает важное, а цикл целей может сжимать контекст, открывать следующий связанный разговор и рассуждать до завершения цели.',
			installFor: 'Установить для {os}',
			seeReasoning: 'Смотреть долгое рассуждение',
			proofOs: 'OS',
			proofOsText: 'автоопределение',
			proofReasoning: 'часы+',
			proofReasoningText: 'рассуждение',
			proofMemory: 'граф',
			proofMemoryText: 'память'
		},
		install: {
			eyebrow: 'Установите последнюю версию',
			title: 'Запустите Anda локально',
			detected: 'обнаружено: {os}',
			chooseOs: 'выберите OS',
			tabAria: 'Способ установки по операционной системе',
			copy: 'Копировать',
			copied: 'Скопировано',
			copyFailed: 'Не удалось скопировать',
			copyAria: 'Скопировать команду установки',
			commandAria: 'Скопировать команду установки',
			alternative: 'Альтернатива: {method}',
			steps: ['Установите релиз', 'Добавьте или экспортируйте ключ', 'Запустите'],
			requiresPrefix:
				'Нужен хотя бы один API-ключ провайдера модели в config или env. При первом запуске Anda создает',
			requiresSuffix: '.',
			options: {
				macos: {
					label: 'macOS',
					commandLabel: 'Shell-скрипт',
					note: 'Скрипт установки загружает последний релиз и выбранные skills для macOS.',
					fallbackLabel: 'Homebrew'
				},
				windows: {
					label: 'Windows',
					commandLabel: 'PowerShell',
					note: 'Запустите это в PowerShell, затем откройте новый терминал и стартуйте Anda.'
				},
				linux: {
					label: 'Linux',
					commandLabel: 'Shell-скрипт',
					note: 'Скрипт установки скачает последний релиз для вашей локальной среды.'
				}
			}
		},
		reasoning: {
			badge: 'Долгое рассуждение',
			title: 'Цель может двигаться дальше, когда обычный чат уже исчерпал бы путь.',
			body: 'Anda может поддерживать одну сессию через множество связанных разговоров. Когда контекст растет, он сжимает текущее состояние, сохраняет цель и продолжает рассуждать почти без практического ограничения по времени или числу ходов.',
			panelTitle: 'Цикл сессии Anda',
			panelStatus: 'goal://active',
			phases: ['рассуждать', 'сжимать', 'продолжать'],
			signals: [
				{ label: 'цель', value: 'активна', level: 92 },
				{ label: 'контекст', value: 'сжат', level: 84 },
				{ label: 'память', value: 'формируется', level: 76 },
				{ label: 'инструменты', value: 'готовы', level: 68 }
			],
			events: [
				{ time: '00:01', phase: 'цель', detail: 'цель принята, сессия открыта' },
				{ time: '47:18', phase: 'сжатие', detail: 'контекст передан в следующий разговор' },
				{ time: 'часы+', phase: 'продолжение', detail: 'рассуждение продолжается до результата' }
			],
			cards: [
				{
					label: 'Цикл цели',
					title: 'Продолжает работать после одного ответа',
					detail:
						'Дайте Anda цель: он может проверять прогресс, продолжать и ждать следующего полезного действия вместо остановки на фиксированном числе ходов.'
				},
				{
					label: 'Передача контекста',
					title: 'Сжимает и несет нить дальше',
					detail:
						'Когда разговор становится большим, Anda резюмирует живое состояние, открывает следующий связанный разговор и продолжает ту же сессию.'
				},
				{
					label: 'Формирование памяти',
					title: 'Учится на успешной работе',
					detail:
						'Полезные ходы попадают в Hippocampus, где связи, решения, предпочтения и линии времени становятся доступным для вызова контекстом.'
				}
			]
		},
		memory: {
			badge: 'Anda Hippocampus',
			title: 'Память делает работу лучше с каждым разом.',
			body: 'Hippocampus превращает полезные разговоры в живой Cognitive Nexus: людей, проекты, решения, предпочтения, события и отношения между ними.',
			formationTitle: 'Формирование',
			formationBody:
				'Я превращаю успешные ходы, файлы, каналы и результаты инструментов в воспоминания с источником, временем и смыслом.',
			recallTitle: 'Вызов',
			recallBody:
				'Когда цели нужна история, я извлекаю контекст, который объясняет текущую задачу, а не просто ближайший текст.',
			maintenanceTitle: 'Поддержка',
			maintenanceBody:
				'Я консолидирую, объединяю, ослабляю и сохраняю линии времени, чтобы прочное знание оставалось доступным, а старый шум исчезал.'
		},
		work: {
			contextRoutes: 'маршруты контекста',
			memoryRoute: 'anda://memory',
			badge: 'Где я работаю',
			title: 'Принесите долгую память туда, где рождается ваш контекст.',
			body: 'Я могу жить в терминале, подключаться к чат-каналам, слушать и говорить голосом, читать файлы, запускать инструменты и вести запланированную работу, пока та же нить памяти следует дальше.',
			surfaces: [
				{
					label: 'цели',
					detail: 'Долгие задачи остаются привязаны к сессии, пока результат не станет реальным.'
				},
				{ label: 'голос', detail: 'Речевой контекст может стать частью той же рабочей памяти.' },
				{
					label: 'shell',
					detail: 'Локальные команды и файлы дают памяти конкретный материал для рассуждения.'
				},
				{
					label: 'каналы',
					detail: 'Контекст, рожденный в командах, может следовать за работой, а не исчезать.'
				}
			],
			cards: [
				{
					title: 'Локальный shell',
					detail:
						'Приземленное рабочее пространство для команд, файлов, экспериментов и многошаговой автоматизации.'
				},
				{
					title: 'Многоканальный чат',
					detail: 'Разговоры вокруг работы могут стать частью рабочей нити.'
				},
				{
					title: 'Ваши ключи и модели',
					detail: 'Подключите провайдера, которому доверяете, и держите runtime рядом с машиной.'
				},
				{
					title: 'Проверяемая память',
					detail: 'Открытые компоненты делают форму моего мозга видимой и пригодной для аудита.'
				}
			]
		},
		start: {
			badge: 'После установки',
			title: 'Запускайте Anda из любого терминала с ключом модели.',
			bodyPrefix:
				'Для первого запуска используйте ключ в переменной окружения или сохраните ключи в',
			bodySuffix: 'для следующих сессий.',
			quickStart: 'Быстрый старт',
			meetHippocampus: 'Познакомиться с Hippocampus',
			terminalLabel: 'команда запуска',
			sourceComment: 'Запустить сразу с ключом окружения:',
			goalComment: 'Или сохраните api_key в config.yaml и запустите:',
			localRuntime: 'локальный runtime',
			durableThread: 'долгая нить',
			inspectableBrain: 'проверяемый мозг'
		}
	},
	ar: {
		meta: {
			title: 'ثبّت Anda Bot - وكيل ذاكرة للاستدلال الطويل',
			description:
				'ثبّت Anda Bot، وكيل ذكاء اصطناعي محلي للاستدلال طويل المدى والتعلّم المستمر وذاكرة رسومية يعمل بها Anda Hippocampus.',
			ogTitle: 'ثبّت Anda Bot - وكيل ذاكرة للاستدلال الطويل',
			ogDescription:
				'شغّل Anda محليًا وأعطه أهدافًا تستمر عبر المحادثات المضغوطة، واسترجاع الذاكرة، والأدوات، والملفات، والقنوات.'
		},
		nav: { install: 'التثبيت', reasoning: 'الاستدلال', memory: 'الذاكرة', surfaces: 'بيئات العمل' },
		language: { label: 'اللغة' },
		hero: {
			badge: 'وكيل ذكاء اصطناعي محلي قابل للتثبيت',
			eyebrow: 'تعلّم مستمر · استدلال طويل المدى',
			title: 'ثبّت Anda. امنحه هدفًا يمكنه تجاوز نافذة الدردشة.',
			body: 'أنا الوكيل المحلي الذي يواصل التعلّم أثناء العمل. يتذكر دماغي Hippocampus ما يهم، ويمكن لحلقة الأهداف ضغط السياق وفتح المحادثة المرتبطة التالية ومواصلة الاستدلال حتى يكتمل الهدف.',
			installFor: 'ثبّت لـ {os}',
			seeReasoning: 'اعرض الاستدلال الطويل',
			proofOs: 'النظام',
			proofOsText: 'اكتشاف تلقائي',
			proofReasoning: 'ساعات+',
			proofReasoningText: 'استدلال',
			proofMemory: 'رسم بياني',
			proofMemoryText: 'ذاكرة'
		},
		install: {
			eyebrow: 'ثبّت أحدث إصدار',
			title: 'شغّل Anda محليًا',
			detected: 'تم اكتشاف {os}',
			chooseOs: 'اختر النظام',
			tabAria: 'طريقة التثبيت حسب نظام التشغيل',
			copy: 'نسخ',
			copied: 'تم النسخ',
			copyFailed: 'تعذّر النسخ',
			copyAria: 'نسخ أمر التثبيت',
			commandAria: 'انسخ أمر التثبيت',
			alternative: 'بديل: {method}',
			steps: ['ثبّت الإصدار', 'أضف المفتاح أو صدّره', 'شغّل'],
			requiresPrefix: 'يتطلب مفتاح API واحدًا على الأقل لمزوّد نموذج في config أو env. ينشئ Anda',
			requiresSuffix: 'عند أول تشغيل.',
			options: {
				macos: {
					label: 'macOS',
					commandLabel: 'سكربت Shell',
					note: 'يثبت سكربت التثبيت أحدث إصدار و skills المنتقاة على macOS.',
					fallbackLabel: 'Homebrew'
				},
				windows: {
					label: 'Windows',
					commandLabel: 'PowerShell',
					note: 'شغّل هذا في PowerShell، ثم افتح طرفية جديدة وابدأ Anda.'
				},
				linux: {
					label: 'Linux',
					commandLabel: 'سكربت Shell',
					note: 'يجلب سكربت التثبيت أحدث إصدار لبيئة التشغيل المحلية لديك.'
				}
			}
		},
		reasoning: {
			badge: 'استدلال طويل المدى',
			title: 'يمكن للهدف أن يواصل الحركة بعد أن تنفد المحادثة العادية من المسار.',
			body: 'يمكن لـ Anda إبقاء الجلسة حيّة عبر كثير من المحادثات المرتبطة. عندما يكبر السياق، يضغط الحالة الحالية، ويحافظ على الهدف، ويواصل الاستدلال تقريبًا بلا حد عملي للمدة أو عدد الجولات.',
			panelTitle: 'حلقة جلسة Anda',
			panelStatus: 'goal://active',
			phases: ['استدلال', 'ضغط', 'متابعة'],
			signals: [
				{ label: 'الهدف', value: 'نشط', level: 92 },
				{ label: 'السياق', value: 'مضغوط', level: 84 },
				{ label: 'الذاكرة', value: 'تتشكل', level: 76 },
				{ label: 'الأدوات', value: 'جاهزة', level: 68 }
			],
			events: [
				{ time: '00:01', phase: 'هدف', detail: 'قُبل الهدف وفتحت الجلسة' },
				{ time: '47:18', phase: 'ضغط', detail: 'لُخّص السياق في المحادثة التالية' },
				{ time: 'ساعات+', phase: 'متابعة', detail: 'يستأنف الاستدلال حتى يكتمل الهدف' }
			],
			cards: [
				{
					label: 'حلقة الهدف',
					title: 'يواصل العمل بعد إجابة واحدة',
					detail:
						'امنح Anda هدفًا، فيمكنه فحص التقدم والمتابعة وانتظار الإجراء المفيد التالي بدل التوقف عند عدد ثابت من الجولات.'
				},
				{
					label: 'تسليم السياق',
					title: 'يضغط الخيط ويحمله للأمام',
					detail:
						'عندما تكبر المحادثة، يلخص Anda الحالة الحية ويفتح المحادثة المرتبطة التالية ويواصل الجلسة نفسها.'
				},
				{
					label: 'تكوين الذاكرة',
					title: 'يتعلم من العمل الناجح',
					detail:
						'تتدفق الجولات المفيدة إلى Hippocampus، حيث تصبح العلاقات والقرارات والتفضيلات والجداول الزمنية سياقًا قابلًا للاسترجاع.'
				}
			]
		},
		memory: {
			badge: 'Anda Hippocampus',
			title: 'الذاكرة تجعل العمل أفضل في كل مرة.',
			body: 'يحوّل Hippocampus المحادثات المفيدة إلى Cognitive Nexus حي: أشخاص، ومشاريع، وقرارات، وتفضيلات، وأحداث، والعلاقات بينها.',
			formationTitle: 'التكوين',
			formationBody:
				'أحوّل الجولات الناجحة والملفات والقنوات ونتائج الأدوات إلى ذكريات لها مصدر ووقت ومعنى.',
			recallTitle: 'الاسترجاع',
			recallBody:
				'عندما يحتاج الهدف إلى تاريخ، أسترجع السياق الذي يشرح المهمة الحالية بدل مطابقة أقرب نص فقط.',
			maintenanceTitle: 'الصيانة',
			maintenanceBody:
				'أدمج وأوحّد وأخفّض الضجيج وأحافظ على الجداول الزمنية كي تبقى المعرفة الدائمة قابلة للوصول بينما يتلاشى الضجيج القديم.'
		},
		work: {
			contextRoutes: 'مسارات السياق',
			memoryRoute: 'anda://memory',
			badge: 'أين أعمل',
			title: 'اجلب الذاكرة الطويلة إلى الأماكن التي يولد فيها سياقك.',
			body: 'يمكنني العيش في الطرفية، والانضمام إلى قنوات الدردشة، والاستماع والتحدث بالصوت، وقراءة الملفات، وتشغيل الأدوات، وتحريك الأعمال المجدولة بينما يتبعها خيط الذاكرة نفسه.',
			surfaces: [
				{ label: 'الأهداف', detail: 'تبقى المهام الطويلة مرتبطة بجلسة حتى يصبح الناتج حقيقيًا.' },
				{ label: 'الصوت', detail: 'يمكن للسياق المنطوق أن يصبح جزءًا من ذاكرة العمل نفسها.' },
				{
					label: 'الطرفية',
					detail: 'تعطي الأوامر والملفات المحلية للذاكرة مادة ملموسة للاستدلال.'
				},
				{ label: 'القنوات', detail: 'يمكن للسياق الذي يولد في الفرق أن يتبع العمل بدل أن يختفي.' }
			],
			cards: [
				{
					title: 'Shell محلي أولًا',
					detail: 'مساحة عمل ملموسة للأوامر والملفات والتجارب والأتمتة متعددة الخطوات.'
				},
				{
					title: 'دردشة متعددة القنوات',
					detail: 'يمكن للمحادثات حول العمل أن تصبح جزءًا من الخيط العامل.'
				},
				{
					title: 'مفاتيحك ونماذجك',
					detail: 'صل المزوّد الذي تثق به وأبق بيئة التشغيل قريبة من جهازك.'
				},
				{
					title: 'ذاكرة قابلة للفحص',
					detail: 'تحافظ المكونات مفتوحة المصدر على شكل دماغي مرئيًا وقابلًا للتدقيق.'
				}
			]
		},
		start: {
			badge: 'بعد التثبيت',
			title: 'شغّل Anda من أي طرفية باستخدام مفتاح نموذج.',
			bodyPrefix: 'استخدم مفتاحًا في متغير بيئة عند التشغيل الأول، أو احفظ مفاتيح المزوّد في',
			bodySuffix: 'للجلسات التالية.',
			quickStart: 'البدء السريع',
			meetHippocampus: 'تعرّف على Hippocampus',
			terminalLabel: 'أمر التشغيل',
			sourceComment: 'ابدأ الآن بمفتاح بيئة:',
			goalComment: 'أو احفظ api_key في config.yaml ثم شغّل:',
			localRuntime: 'تشغيل محلي',
			durableThread: 'خيط دائم',
			inspectableBrain: 'دماغ قابل للفحص'
		}
	}
};

export function isLocale(value: string | null | undefined): value is Locale {
	return Boolean(value && value in localeMeta);
}

export function detectLocale(languages: readonly string[]): Locale {
	for (const language of languages) {
		const tag = language.toLowerCase();
		const base = tag.split('-')[0];
		if (isLocale(base)) return base;
	}

	return fallbackLocale;
}
