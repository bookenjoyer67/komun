/** The post shape, written once: the card, the detail route, the aggregator and the tests all speak this. */

/** Mirrors `komun_core::models::post::PostKind` (`chk_posts_kind`). */
export type PostKind = 'resource' | 'need' | 'offer' | 'listing' | 'want';

/** Mirrors `komun_core::models::post::Urgency`. */
export type Urgency = 'critical' | 'high' | 'medium' | 'low';

export interface PostLike {
	id: string;
	kind: PostKind;
	category: string;
	title: string;
	body?: string;
	location_name?: string;
	location_lat?: number;
	location_lon?: number;
	urgency?: Urgency | string;
	status?: string;
	author_id?: string;
	author_name?: string;
	images?: string[];
	contact_method?: string;
	verified_by?: string | null;
	created_at: string;
	/** Present only on posts gathered from other servers by the aggregator. */
	server_url?: string;
	server_name?: string;
	server_location?: string;
}
