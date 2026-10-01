const en = {
  connectModel: 'Connect a model',
  setupWelcome: 'Welcome to Anda',
  setupIntro:
    'Connect an AI model to start chatting and working on tasks. You can change it later in Settings.',
  setupChatGpt: 'Continue with ChatGPT',
  setupChatGptHint: 'Use the models available to your account. No API key needed.',
  setupProviders: 'Use another model service',
  setupProvidersHint: 'Choose a preset and add your API key.',
  setupLater: 'Set up later',
  setupBack: 'Back',
  setupPreset: 'Choose a model',
  setupApiKey: 'API key',
  setupKeyHint:
    'Get an API key from the selected service. Its API usage is billed by that service.',
  setupKeyRequired: 'Enter an API key to continue.',
  setupConnect: 'Connect and start',
  setupActivating: 'Enabling your model…',
  setupLoaded: 'Your model is ready',
  setupLoadedHint:
    'The default model has been loaded. Send your first message to start a conversation.',
  setupStart: 'Start chatting',
  setupRetry: 'Try again',
  setupUnavailable:
    'The model is not ready yet. Retry, or open Agent configuration to check the saved settings.',
  setupRepair: 'Complete your configuration to start chatting.',
  setupAdvanced: 'Open Agent configuration',
  setupPresetConflict:
    'This model already uses custom settings. Edit it in Agent configuration to preserve those settings.',
  setupInvalidConfig: 'The configuration could not be read. Open Agent configuration to repair it.',
  setupMissingModel: 'Connect an AI model to start chatting.',
  setupWaitingHint:
    'Your settings have been saved. Waiting for the local service to load the model.',
  setupNoModels: 'This account has no available models. Reconnect or choose another model service.'
}

export const modelSetupMessages: Record<string, Record<keyof typeof en, string>> = {
  en,
  zh_CN: {
    connectModel: '连接模型',
    setupWelcome: '欢迎使用 Anda',
    setupIntro: '连接一个 AI 模型，就能开始聊天、处理任务。之后可以随时在设置中更换。',
    setupChatGpt: '使用 ChatGPT 登录',
    setupChatGptHint: '使用账号支持的模型，无需填写 API Key。',
    setupProviders: '连接其他模型服务',
    setupProvidersHint: '选择一个预置模型，填写 API Key 即可。',
    setupLater: '稍后设置',
    setupBack: '返回',
    setupPreset: '选择模型',
    setupApiKey: 'API Key',
    setupKeyHint: '请从所选服务获取 API Key，API 用量由该服务计费。',
    setupKeyRequired: '请填写 API Key。',
    setupConnect: '连接并开始',
    setupActivating: '正在启用模型…',
    setupLoaded: '模型已就绪',
    setupLoadedHint: '默认模型已加载。发送第一条消息，开始对话吧。',
    setupStart: '开始聊天',
    setupRetry: '重试',
    setupUnavailable: '模型尚未就绪。请重试，或打开 Agent 配置检查已保存的设置。',
    setupRepair: '完成配置后即可开始对话。',
    setupAdvanced: '打开 Agent 配置',
    setupPresetConflict: '该模型已有自定义配置。请在 Agent 配置中编辑，以保留现有设置。',
    setupInvalidConfig: '无法读取配置，请打开 Agent 配置修复。',
    setupMissingModel: '连接 AI 模型后即可开始对话。',
    setupWaitingHint: '设置已保存，正在等待本地服务加载模型。',
    setupNoModels: '该账号暂无可用模型，请重新连接或选择其他模型服务。'
  },
  fr: {
    connectModel: 'Connecter un modèle',
    setupWelcome: 'Bienvenue dans Anda',
    setupIntro:
      'Connectez un modèle IA pour discuter et accomplir vos tâches. Vous pourrez le changer dans les paramètres.',
    setupChatGpt: 'Continuer avec ChatGPT',
    setupChatGptHint: 'Utilisez les modèles de votre compte, sans clé API.',
    setupProviders: 'Utiliser un autre service',
    setupProvidersHint: 'Choisissez un modèle prédéfini et ajoutez votre clé API.',
    setupLater: 'Configurer plus tard',
    setupBack: 'Retour',
    setupPreset: 'Choisir un modèle',
    setupApiKey: 'Clé API',
    setupKeyHint:
      'Obtenez une clé API auprès du service choisi. Ce service facture votre utilisation de son API.',
    setupKeyRequired: 'Saisissez une clé API pour continuer.',
    setupConnect: 'Connecter et démarrer',
    setupActivating: 'Activation du modèle…',
    setupLoaded: 'Votre modèle est prêt',
    setupLoadedHint:
      'Le modèle par défaut est chargé. Envoyez votre premier message pour commencer.',
    setupStart: 'Commencer à discuter',
    setupRetry: 'Réessayer',
    setupUnavailable:
      'Le modèle n’est pas encore prêt. Réessayez ou vérifiez les paramètres enregistrés dans la configuration de l’agent.',
    setupRepair: 'Terminez la configuration pour commencer à discuter.',
    setupAdvanced: 'Ouvrir la configuration de l’agent',
    setupPresetConflict:
      'Ce modèle utilise déjà des paramètres personnalisés. Modifiez-le dans la configuration de l’agent pour les préserver.',
    setupInvalidConfig:
      'Impossible de lire la configuration. Ouvrez la configuration de l’agent pour la corriger.',
    setupMissingModel: 'Connectez un modèle IA pour commencer à discuter.',
    setupWaitingHint: 'Paramètres enregistrés. Le service local charge le modèle.',
    setupNoModels:
      'Aucun modèle disponible pour ce compte. Reconnectez-vous ou choisissez un autre service.'
  },
  es: {
    connectModel: 'Conectar un modelo',
    setupWelcome: 'Te damos la bienvenida a Anda',
    setupIntro:
      'Conecta un modelo de IA para conversar y realizar tareas. Puedes cambiarlo después en Ajustes.',
    setupChatGpt: 'Continuar con ChatGPT',
    setupChatGptHint: 'Usa los modelos de tu cuenta sin una clave API.',
    setupProviders: 'Usar otro servicio de modelos',
    setupProvidersHint: 'Elige un modelo predefinido y añade tu clave API.',
    setupLater: 'Configurar más tarde',
    setupBack: 'Volver',
    setupPreset: 'Elegir un modelo',
    setupApiKey: 'Clave API',
    setupKeyHint:
      'Obtén una clave API del servicio elegido. Ese servicio cobra por el uso de su API.',
    setupKeyRequired: 'Introduce una clave API para continuar.',
    setupConnect: 'Conectar y empezar',
    setupActivating: 'Activando el modelo…',
    setupLoaded: 'Tu modelo está listo',
    setupLoadedHint: 'El modelo predeterminado está cargado. Envía tu primer mensaje para empezar.',
    setupStart: 'Empezar a conversar',
    setupRetry: 'Reintentar',
    setupUnavailable:
      'El modelo aún no está listo. Reintenta o revisa los ajustes guardados en la configuración del agente.',
    setupRepair: 'Completa la configuración para empezar a conversar.',
    setupAdvanced: 'Abrir configuración del agente',
    setupPresetConflict:
      'Este modelo ya tiene ajustes personalizados. Edítalo en la configuración del agente para conservarlos.',
    setupInvalidConfig:
      'No se pudo leer la configuración. Abre la configuración del agente para corregirla.',
    setupMissingModel: 'Conecta un modelo de IA para empezar a conversar.',
    setupWaitingHint: 'Ajustes guardados. Esperando a que el servicio local cargue el modelo.',
    setupNoModels:
      'Esta cuenta no tiene modelos disponibles. Vuelve a conectarte o elige otro servicio.'
  },
  ru: {
    connectModel: 'Подключить модель',
    setupWelcome: 'Добро пожаловать в Anda',
    setupIntro:
      'Подключите модель ИИ, чтобы общаться и выполнять задачи. Позже её можно изменить в настройках.',
    setupChatGpt: 'Войти через ChatGPT',
    setupChatGptHint: 'Используйте модели своего аккаунта без API-ключа.',
    setupProviders: 'Подключить другой сервис',
    setupProvidersHint: 'Выберите готовую модель и введите API-ключ.',
    setupLater: 'Настроить позже',
    setupBack: 'Назад',
    setupPreset: 'Выберите модель',
    setupApiKey: 'API-ключ',
    setupKeyHint:
      'Получите API-ключ у выбранного сервиса. Использование API оплачивается этому сервису.',
    setupKeyRequired: 'Введите API-ключ для продолжения.',
    setupConnect: 'Подключить и начать',
    setupActivating: 'Включение модели…',
    setupLoaded: 'Модель готова',
    setupLoadedHint: 'Модель по умолчанию загружена. Отправьте первое сообщение, чтобы начать.',
    setupStart: 'Начать общение',
    setupRetry: 'Повторить',
    setupUnavailable:
      'Модель ещё не готова. Повторите попытку или проверьте сохранённые настройки в конфигурации агента.',
    setupRepair: 'Завершите настройку, чтобы начать общение.',
    setupAdvanced: 'Открыть конфигурацию агента',
    setupPresetConflict:
      'У этой модели уже есть собственные настройки. Измените её в конфигурации агента, чтобы сохранить их.',
    setupInvalidConfig:
      'Не удалось прочитать конфигурацию. Откройте конфигурацию агента для исправления.',
    setupMissingModel: 'Подключите модель ИИ, чтобы начать общение.',
    setupWaitingHint: 'Настройки сохранены. Ожидание загрузки модели локальной службой.',
    setupNoModels:
      'В этом аккаунте нет доступных моделей. Подключитесь заново или выберите другой сервис.'
  },
  ar: {
    connectModel: 'ربط نموذج',
    setupWelcome: 'مرحبًا بك في Anda',
    setupIntro:
      'اربط نموذج ذكاء اصطناعي لبدء المحادثة وإنجاز المهام. يمكنك تغييره لاحقًا من الإعدادات.',
    setupChatGpt: 'المتابعة باستخدام ChatGPT',
    setupChatGptHint: 'استخدم النماذج المتاحة لحسابك دون مفتاح API.',
    setupProviders: 'استخدام خدمة نماذج أخرى',
    setupProvidersHint: 'اختر نموذجًا جاهزًا وأضف مفتاح API.',
    setupLater: 'الإعداد لاحقًا',
    setupBack: 'رجوع',
    setupPreset: 'اختيار نموذج',
    setupApiKey: 'مفتاح API',
    setupKeyHint:
      'احصل على مفتاح API من الخدمة المختارة. تتولى تلك الخدمة احتساب تكلفة استخدام API.',
    setupKeyRequired: 'أدخل مفتاح API للمتابعة.',
    setupConnect: 'الربط والبدء',
    setupActivating: 'جارٍ تفعيل النموذج…',
    setupLoaded: 'النموذج جاهز',
    setupLoadedHint: 'تم تحميل النموذج الافتراضي. أرسل رسالتك الأولى لبدء المحادثة.',
    setupStart: 'بدء المحادثة',
    setupRetry: 'إعادة المحاولة',
    setupUnavailable:
      'النموذج غير جاهز بعد. أعد المحاولة أو افتح إعدادات الوكيل للتحقق من الإعدادات المحفوظة.',
    setupRepair: 'أكمل الإعداد لبدء المحادثة.',
    setupAdvanced: 'فتح إعدادات الوكيل',
    setupPresetConflict:
      'يستخدم هذا النموذج إعدادات مخصصة بالفعل. عدّله من إعدادات الوكيل للحفاظ عليها.',
    setupInvalidConfig: 'تعذرت قراءة الإعدادات. افتح إعدادات الوكيل لإصلاحها.',
    setupMissingModel: 'اربط نموذج ذكاء اصطناعي لبدء المحادثة.',
    setupWaitingHint: 'تم حفظ الإعدادات. في انتظار تحميل النموذج بواسطة الخدمة المحلية.',
    setupNoModels: 'لا توجد نماذج متاحة لهذا الحساب. أعد الاتصال أو اختر خدمة أخرى.'
  }
}
