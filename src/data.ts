export type Scope = "個人" | "チーム" | "組織";
export type Goal = {
  id: string;
  title: string;
  subtitle: string;
  scope: Scope;
  icon: string;
  purpose: string;
  progress: number | null;
  next: string;
  memo: string;
  startDate?: string | null;
  dueDate?: string | null;
  state?: string;
};
export type Task = {
  id: string;
  title: string;
  scope: Scope;
  time: string;
  done: boolean;
  date?: string | null;
  recurring?: boolean;
  state?: string;
  dueDate?: string | null;
  assigneeId?: string | null;
  priority?: "low" | "medium" | "high" | "urgent" | null;
  dueStatus?: "overdue" | "today" | "soon" | null;
};
export type Initiative = {
  id: string;
  goalId: string;
  title: string;
  icon: string;
  progress: number | null;
};
export const scopeClass: Record<Scope, string> = {
  個人: "blue",
  チーム: "purple",
  組織: "green",
};
export const initialGoals: Goal[] = [
  {
    id: "english",
    title: "英語で話せるようになる",
    subtitle: "海外の人と自由に会話できる",
    scope: "個人",
    icon: "airplane",
    purpose:
      "言葉の壁を越えて、世界中の人とつながる。\n自分の考えを、自分の言葉で伝えられるように。",
    progress: 45,
    next: "英会話を30分学習する",
    memo: "毎日少しずつ、楽しみながら続ける。\n次の海外旅行で、現地の人と会話してみたい。",
  },
  {
    id: "event",
    title: "地域イベントを開催する",
    subtitle: "人がつながる場をつくり、地域の魅力を再発見する",
    scope: "チーム",
    icon: "mountains",
    purpose:
      "地域の人たちが気軽に集まり、つながり、\n新しい挑戦が生まれるきっかけをつくる。",
    progress: 40,
    next: "会場の候補を3つに絞って、下見を予約する",
    memo: "地域のカフェや商店街とも連携できそう。\n小さく始めて、継続的なイベントに育てたい。",
  },
  {
    id: "business",
    title: "新規事業を育てる",
    subtitle: "社会に新しい価値を届ける",
    scope: "組織",
    icon: "cube",
    purpose:
      "身近な課題から、新しい可能性を見つける。\n本当に必要とされるサービスを形にする。",
    progress: 35,
    next: "インタビュー候補者に日程を確認する",
    memo: "まずは10人のリアルな声を聞いてみる。\n小さな仮説検証を重ねて、価値を確かめたい。",
  },
];
export const initialInitiatives: Initiative[] = [
  {
    id: "learn",
    goalId: "english",
    title: "学習を週3回続ける",
    icon: "book",
    progress: 60,
  },
  {
    id: "travel",
    goalId: "english",
    title: "海外旅行で\n実際に使ってみる",
    icon: "airplane",
    progress: 30,
  },
  {
    id: "venue",
    goalId: "event",
    title: "会場を決める",
    icon: "calendar",
    progress: 80,
  },
  {
    id: "pr",
    goalId: "event",
    title: "広報・集客を行う",
    icon: "users",
    progress: 40,
  },
  {
    id: "interview",
    goalId: "business",
    title: "インタビューを\n10件行う",
    icon: "bulb",
    progress: 50,
  },
  {
    id: "mvp",
    goalId: "business",
    title: "MVPを作る",
    icon: "cube",
    progress: 30,
  },
];
export const initialTasks: Task[] = [
  {
    id: "t1",
    title: "英単語を30分学習する",
    scope: "個人",
    time: "09:00",
    done: true,
  },
  {
    id: "t2",
    title: "会場の候補を調べる",
    scope: "チーム",
    time: "11:00",
    done: false,
  },
  {
    id: "t3",
    title: "インタビュー質問を整理する",
    scope: "組織",
    time: "14:00",
    done: false,
  },
  {
    id: "t4",
    title: "読書メモをまとめる",
    scope: "個人",
    time: "20:00",
    done: false,
  },
];
export const templates = [
  {
    title: "自由形式",
    description: "やりたいことから始める",
    icon: "sparkle",
    color: "purple",
  },
  {
    title: "OKR",
    description: "目標と成果で進捗を測る",
    icon: "target",
    color: "blue",
  },
  {
    title: "プロジェクト",
    description: "やることを整理して進める",
    icon: "folder",
    color: "green",
  },
  {
    title: "学習計画",
    description: "スキル・知識を育てる",
    icon: "graduation",
    color: "orange",
  },
  {
    title: "習慣づくり",
    description: "なりたい自分をつくる",
    icon: "repeat",
    color: "pink",
  },
];
export const learnings = [
  "地域の課題を知るには、実際に話を聞くことが一番早い",
  "完璧を目指すより、まずやってみることで見えてくるものがある",
  "英語の学習は、毎日少しでも続けることが効果的",
  "メンバーそれぞれの「やりたいこと」が、チームの推進力になる",
];
