import type { ProfileSectionId } from "./profileTypes";
export type AvatarFeature = {
	id: string;
	group: "source" | "look" | "delivery" | "operation";
	section?: ProfileSectionId;
	destination: "settings" | "renderers" | "logs" | "app";
	ja: string;
	en: string;
	terms: string;
};
export const avatarFeatures: AvatarFeature[] = [
	{
		id: "avatar",
		group: "source",
		section: "avatar",
		destination: "settings",
		ja: "アバター・衣装を選ぶ",
		en: "Choose your avatar",
		terms: "VRM / UNAVATAR / モデル / マニフェスト / 利用条件 / メタデータ / model / manifest / rights",
	},
	{
		id: "motion",
		group: "source",
		section: "motion",
		destination: "settings",
		ja: "動き・表情を受け取る",
		en: "Receive motion and expressions",
		terms: "UN Motion / Zenoh / VMC / 複数購読 / 頭 / 顔 / 手 / 体 / 視線 / ルート移動 / LookAt / root translation",
	},
	{
		id: "physics",
		group: "look",
		section: "physics",
		destination: "settings",
		ja: "髪・服・体の揺れ",
		en: "Hair, clothing and body dynamics",
		terms: "物理 / 揺れもの / コライダー / プリセット / ボーン / グループ別 / 布の拘束 / 追従補助 / 衝突相手 / physics / spring / collider / dynamics / surface constraints / mesh assist",
	},
	{
		id: "lighting",
		group: "look",
		section: "lighting",
		destination: "settings",
		ja: "照明を整える",
		en: "Lighting",
		terms: "ライト / 明るさ / 色 / 方向 / 環境光 / light / intensity / color / direction",
	},
	{
		id: "look",
		group: "look",
		section: "look",
		destination: "settings",
		ja: "画づくり・表現",
		en: "Rendering and presentation",
		terms: "見た目 / 色調 / 輪郭 / 影 / bloom / outline / tone / shadow",
	},
	{
		id: "quality",
		group: "look",
		section: "quality",
		destination: "settings",
		ja: "画質と負荷",
		en: "Quality and performance",
		terms: "FPS / アンチエイリアス / AA / テクスチャ / 圧縮 / 解像度 / GPU / texture / compression",
	},
	{
		id: "camera",
		group: "delivery",
		section: "camera",
		destination: "settings",
		ja: "構図・カメラ",
		en: "Framing and camera",
		terms: "顔 / 胸 / 全身 / レンズ / 焦点距離 / 位置 / 向き / ターゲット / lens / focal length / position / target",
	},
	{
		id: "output",
		group: "delivery",
		section: "output",
		destination: "settings",
		ja: "配信へ映像を送る",
		en: "Send video to your stream",
		terms: "Spout2 / OBS / 出力解像度 / 送信名 / プレビュー / ウィンドウ / output resolution / sender / preview",
	},
	{
		id: "window",
		group: "delivery",
		section: "window",
		destination: "settings",
		ja: "デスクトップでの表示",
		en: "Desktop window",
		terms: "背景 / サイズ / 位置 / 透過 / 最前面 / クリック透過 / 常駐 / size / position / transparent / topmost / click-through",
	},
	{
		id: "identity",
		group: "operation",
		section: "identity",
		destination: "settings",
		ja: "プロファイルの整理",
		en: "Organize profiles",
		terms: "名前 / アイコン / メモ / グループ / 複数同時使用 / name / icon / note / group",
	},
	{
		id: "instances",
		group: "operation",
		destination: "renderers",
		ja: "実行中の個体・衣装・アクション",
		en: "Running instances, wardrobe and actions",
		terms: "起動 / 停止 / 個別操作 / 着替え / スクリーンショット / 診断 / start / stop / wardrobe / actions / screenshot / diagnostics",
	},
	{
		id: "logs",
		group: "operation",
		destination: "logs",
		ja: "ログ・問題の確認",
		en: "Logs and troubleshooting",
		terms: "エラー / 記録 / フィルター / error / logs / filter",
	},
	{
		id: "app",
		group: "operation",
		destination: "app",
		ja: "アプリ全体の設定",
		en: "App preferences",
		terms: "言語 / テーマ / トレイ / 自動起動 / タスクバー / language / theme / tray / startup / taskbar",
	},
];
export function findAvatarFeatures(query: string): AvatarFeature[] {
	const words = query.normalize("NFKC").toLowerCase().trim().split(/\s+/).filter(Boolean);
	return avatarFeatures.filter((f) => words.every((word) => `${f.ja} ${f.en} ${f.terms}`.normalize("NFKC").toLowerCase().includes(word)));
}
