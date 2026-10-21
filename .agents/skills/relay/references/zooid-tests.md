# zooid test inventory

Every test in zooid's suite, and the caravel test that covers the same behavior or why none does.

## api_test.go

- `TestAPIHandler_Authentication` — Obsolete: zooid's REST API is gone; the API's own authentication is `crates/api/tests/auth.rs`.
- `TestAPIHandler_CreateRelay` — Obsolete: a relay's config is its caravel row; `crates/api/tests/wire.rs` a_relay_reads_back_in_the_shape_clients_expect
- `TestAPIHandler_UpdateRelay` — Obsolete: a relay's config is its caravel row, updated through `PUT /relays/{id}`.
- `TestAPIHandler_PatchRelay` — Obsolete: a relay's config is its caravel row, updated through `PUT /relays/{id}`.
- `TestAPIHandler_DeleteRelay` — `crates/core/tests/media.rs` erasing_a_purged_relay_removes_what_it_held
- `TestAPIHandler_ListRelayMembers` — `crates/api/tests/wire.rs` a_relay_reads_back_in_the_shape_clients_expect
- `TestAPIHandler_GetRelayBlob` — `crates/api/tests/relay_data.rs` admins_review_the_feed_and_its_blobs
- `TestAPIHandler_ListModeration` — `crates/core/tests/moderation.rs` the_feed_holds_moderated_events_and_uploads_newest_first
- `TestAPIHandler_ListModeration_Truncated` — `crates/core/tests/moderation.rs` a_full_page_says_it_was_truncated
- `TestCollectModerationItems_OrdersBySeenAt` — `crates/core/tests/moderation.rs` the_feed_window_is_inclusive_at_both_ends
- `TestAPIHandler_ListModeration_RedirectInstance` — Obsolete: redirect hosts are `host_redirect` rows, not instances with events of their own.
- `TestAPIHandler_ListModeration_Unmoderated` — `crates/core/tests/moderation.rs` the_feed_holds_moderated_events_and_uploads_newest_first
- `TestAPIHandler_MethodNotAllowed` — Obsolete: zooid's REST API is gone.
- `TestAPIHandler_InvalidPath` — Obsolete: zooid's REST API is gone.
- `TestAPIHandler_ConfigValidation` — Obsolete: settings are validated by the core's hosting module; `crates/api/tests/wire.rs` validation_failures_carry_a_code
- `TestAPIHandler_RelayMemberWrites` — `crates/relay/tests/access.rs` a_member_added_through_the_api_can_read_straight_away
- `TestAPIHandler_RelayNotifications` — `crates/core/tests/notifications.rs` a_message_is_kept_and_announced
- `TestAPIHandler_RelayClaims` — `crates/core/tests/memberships.rs` an_invite_goes_to_whoever_joins_with_it_first
- `TestNewAPIHandler` — Obsolete: zooid's REST API is gone.
- `TestAPIHandler_GetRelayUsage` — Obsolete: usage is read in process; `crates/core/tests/metering.rs` overage_is_billed_from_measured_media_and_calls
- `TestAPIHandler_LivekitWebhook` — `crates/relay/tests/livekit.rs` a_webhook_must_be_signed_and_name_a_known_call
- `TestAPIHandler_ImportExportRelay` — `crates/api/tests/relay_data.rs` an_owner_exports_one_relay_and_imports_into_another

## blossom_test.go

- `TestBlossomStore_Enable_DeleteWithoutAuthHeaderDoesNotPanic` — `crates/relay/tests/blossom.rs` a_delete_without_a_valid_authorization_is_refused
- `TestBlossomStore_Enable_DeleteWithMalformedAuthHeaderDoesNotPanic` — `crates/relay/tests/blossom.rs` a_delete_without_a_valid_authorization_is_refused
- `TestBlossomStore_Enable_ServesUnrenderableBlobsAsDownloads` — `crates/relay/tests/blossom.rs` unrenderable_blobs_are_served_as_downloads
- `TestBlossomStore_Enable_ExposesRefusalReason` — `crates/relay/tests/blossom.rs` outsiders_are_refused_with_a_reason
- `TestIsInlineBlobType` — `crates/relay/tests/blossom.rs` a_member_uploads_and_anyone_downloads
- `TestBlossomStore_StorageBytes` — `crates/core/tests/media.rs` storage_counts_each_blob_once

## config_test.go

- `TestConfig_IsOwner` — `crates/core/tests/relay_policy.rs` members_and_managers_may_write_and_strangers_may_not
- `TestConfig_IsSelf` — `crates/core/tests/relay_policy.rs` writing_needs_authentication_as_the_author
- `TestValidatePayments` — `crates/core/tests/memberships.rs` a_member_price_must_be_chargeable_and_need_a_claim
- `TestValidateBlossomFileStorage` — Obsolete: storage is platform configuration, not per relay.
- `TestValidateMemberMethods` — Obsolete: member methods follow the relay's members-can-invite flag.
- `TestValidateAnonymousRead` — Obsolete: anonymous reads are a relay setting the API takes as a flag.
- `TestValidateRedirect` — Obsolete: there are no config files.
- `TestValidateRedirectFrom` — Obsolete: redirects are `host_redirect` rows written by the core.
- `TestLoadConfigFromPath_BlossomS3` — Obsolete: there are no config files.

## env_test.go

- `TestLoadEnvReadsDotenv` — Obsolete: the app reads `.env` with dotenvy.
- `TestLoadEnvWithoutDotenv` — Obsolete: the app reads `.env` with dotenvy.

## events_test.go

- `TestEventStore_SaveEvent` — `crates/core/tests/events.rs` a_saved_event_reads_back_exactly
- `TestEventStore_QueryEvents_Basic` — `crates/core/tests/events.rs` queries_match_kinds_authors_ids_tags_and_time
- `TestEventStore_QueryEvents_ByKind` — `crates/core/tests/events.rs` queries_match_kinds_authors_ids_tags_and_time
- `TestEventStore_QueryEvents_ByAuthor` — `crates/core/tests/events.rs` queries_match_kinds_authors_ids_tags_and_time
- `TestEventStore_QueryEvents_ByIDs` — `crates/core/tests/events.rs` queries_match_kinds_authors_ids_tags_and_time
- `TestEventStore_QueryEvents_ByTags` — `crates/core/tests/events.rs` queries_match_kinds_authors_ids_tags_and_time
- `TestEventStore_QueryEvents_TimeRange` — `crates/core/tests/events.rs` queries_match_kinds_authors_ids_tags_and_time
- `TestEventStore_QueryEvents_Limit` — `crates/core/tests/events.rs` limits_cap_results_and_ties_break_on_id
- `TestEventStore_QueryEvents_LimitZero` — `crates/core/tests/events.rs` limits_cap_results_and_ties_break_on_id
- `TestEventStore_QueryEvents_Search` — `crates/core/tests/events.rs` search_matches_words_in_sequence
- `TestEventStore_DeleteEvent` — `crates/core/tests/events.rs` a_deletion_removes_its_authors_events_and_refuses_them_after
- `TestEventStore_ReplaceEvent` — `crates/core/tests/events.rs` a_replaceable_event_keeps_one_newest_version_per_slot
- `TestEventStore_ReplaceEvent_Republish` — `crates/core/tests/events.rs` a_replaceable_event_keeps_one_newest_version_per_slot
- `TestEventStore_ReplaceEvent_OlderEvent` — `crates/core/tests/events.rs` a_replaceable_event_keeps_one_newest_version_per_slot
- `TestEventStore_CountEvents` — `crates/core/tests/events.rs` counts_group_matching_events_by_kind_author_and_room_tags; `crates/relay/tests/protocol.rs` a_count_is_admitted_and_filtered_like_a_subscription
- `TestEventStore_Close` — Obsolete: there is no per-relay store to close.
- `TestEventStore_GetOrCreateApplicationSpecificData` — Obsolete: relay state lives in tables, not app-data events.
- `TestEventStore_QueryEvents_MaxLimitCapsFilterWithNoLimit` — `crates/core/tests/events.rs` limits_cap_results_and_ties_break_on_id
- `TestEventStore_CountEvents_IgnoresFilterLimit` — `crates/core/tests/events.rs` counts_group_matching_events_by_kind_author_and_room_tags
- `TestEventStore_QueryEvents_SearchPhrase` — `crates/core/tests/events.rs` search_matches_words_in_sequence
- `TestEventStore_SaveEvent_ConcurrentDuplicate` — `crates/core/tests/events.rs` a_saved_event_is_a_duplicate_the_second_time
- `TestEventStore_QueryEvents_EmptyFilterFieldMatchesNothing` — `crates/core/tests/events.rs` an_absent_field_matches_everything_and_an_empty_one_nothing
- `TestEventStore_QueryEvents_NilFilterFieldsMatchEverything` — `crates/core/tests/events.rs` an_absent_field_matches_everything_and_an_empty_one_nothing

## groups_test.go

- `TestGetGroupIDFromEvent` — `crates/core/tests/rooms.rs` an_event_names_its_room_by_d_for_room_state_and_h_otherwise
- `TestGroupStore_UpdatePins` — `crates/core/tests/rooms.rs` only_moderators_change_a_room_and_nobody_writes_its_state_directly
- `TestGroupStore_AddMember_IsIdempotent` — `crates/core/tests/rooms.rs` joining_and_leaving_a_room_is_published_by_the_relay
- `TestGroupStore_RemoveMember_SkipsNonMember` — `crates/core/tests/rooms.rs` joining_and_leaving_a_room_is_published_by_the_relay
- `TestGroupStore_CheckWrite_PutPins` — `crates/core/tests/rooms.rs` only_moderators_change_a_room_and_nobody_writes_its_state_directly
- `TestGroupStore_DeleteGroup_RemovesPins` — `crates/core/tests/rooms.rs` deleting_a_room_removes_it_and_everything_in_it
- `TestGroupStore_DeleteEvents` — `crates/core/tests/rooms.rs` deleting_events_stays_within_the_room_and_spares_its_state
- `TestGroupStore_DeleteEvents_SkipsAnotherGroup` — `crates/core/tests/rooms.rs` deleting_events_stays_within_the_room_and_spares_its_state
- `TestGroupStore_DeleteEvents_SkipsGroupState` — `crates/core/tests/rooms.rs` deleting_events_stays_within_the_room_and_spares_its_state
- `TestGroupStore_CheckWrite_DeleteEvent` — `crates/core/tests/rooms.rs` a_relay_method_holder_may_not_moderate_rooms_but_a_room_admin_may
- `TestGroupStore_IsInvited` — `crates/core/tests/rooms.rs` a_closed_room_takes_joins_only_with_an_invite_and_a_hidden_one_hides
- `TestGroupStore_CheckWrite_HiddenGroupJoinWithInvite` — `crates/core/tests/rooms.rs` a_closed_room_takes_joins_only_with_an_invite_and_a_hidden_one_hides
- `TestGroupStore_CacheInvalidation` — `crates/relay/tests/access.rs` a_private_room_is_read_only_by_its_members_stored_and_live (the snapshot reloads after every room change)

## instance_test.go

- `TestMakeInstance_Payments` — `crates/relay/tests/payments.rs` a_paid_relay_states_its_price_and_where_to_pay
- `TestMakeInstance_NoPayments` — `crates/relay/tests/payments.rs` a_paid_relay_states_its_price_and_where_to_pay
- `TestMakeInstance_Redirect` — Obsolete: redirect-only configs became `host_redirect` rows, covered with the hosts in `crates/relay/tests/protocol.rs`.
- `TestInstance_AllowRecipientEvent` — `crates/core/tests/relay_policy.rs` a_gift_wrap_or_zap_for_a_member_needs_no_authentication
- `TestInstance_PreventBroadcast_PrivateGroup` — `crates/relay/tests/access.rs` a_private_room_is_read_only_by_its_members_stored_and_live
- `TestInstance_OnRequest_Anonymous` — `crates/core/tests/relay_policy.rs` reading_needs_authentication_unless_anonymous_reads_are_on
- `TestInstance_QueryStored_Anonymous` — `crates/relay/tests/protocol.rs` an_anonymous_relay_serves_readers_who_never_authenticate
- `TestInstance_IsInternalEvent` — `crates/core/tests/relay_policy.rs` kinds_the_relay_writes_itself_are_refused
- `TestInstance_OnEventSaved_ClosedGroupJoinWithInvite` — `crates/core/tests/rooms.rs` a_closed_room_takes_joins_only_with_an_invite_and_a_hidden_one_hides
- `TestInstance_CachesSeeClientWrites` — `crates/relay/tests/access.rs` a_private_room_is_read_only_by_its_members_stored_and_live
- `TestInstance_BanEventInvalidatesCaches` — `crates/core/tests/access.rs` banning_an_event_deletes_it_until_it_is_allowed
- `TestInstance_OnEvent_CreatedAtUpperLimit` — `crates/core/tests/relay_policy.rs` events_dated_too_far_ahead_are_refused
- `TestMakeInstance_CreatedAtUpperLimit` — `crates/relay/tests/protocol.rs` the_information_document_describes_the_relay
- `TestInstance_JoinIsLoggedAgainstItsClaim` — `crates/relay/tests/access.rs` a_claim_lets_a_stranger_join_and_read

## lib_test.go

- `TestDispatchRedirectFrom` — `crates/relay/tests/protocol.rs` a_verified_custom_domain_and_the_canonical_host_both_serve_the_relay
- `TestDispatch_IgnoresInactiveInstances` — `crates/relay/tests/protocol.rs` a_deactivated_relay_closes_its_connections_and_stops_answering

## livekit_test.go

- `TestLivekitStore_ParticipantSeconds` — `crates/core/tests/media.rs` the_call_clock_sums_each_session_once
- `TestLivekitStore_TokenHandler` — `crates/relay/tests/livekit.rs` tokens_are_refused_to_those_who_may_not_join
- `TestLivekitStore_WebhookHandler` — `crates/relay/tests/livekit.rs` a_departure_is_metered_once_and_the_call_published

## main_test.go

- `TestMain` — Obsolete: `caravel_db::testing` creates a database per test binary.

## management_test.go

- `TestValidateJoinRequest_PaidRelay` — `crates/relay/tests/payments.rs` a_paid_relay_takes_joins_only_with_a_claim
- `TestValidateJoinRequest_PaidRelayTakesAnInvite` — `crates/relay/tests/payments.rs` a_paid_relay_takes_joins_only_with_a_claim
- `TestManagementStore_MembershipChangesAreIdempotent` — `crates/core/tests/access.rs` membership_changes_are_idempotent_and_published
- `TestManagementStore_BanPubkey` — `crates/core/tests/access.rs` banning_a_pubkey_removes_it_and_everything_it_wrote
- `TestManagementStore_AllowPubkey` — `crates/core/tests/access.rs` banning_a_pubkey_removes_it_and_everything_it_wrote
- `TestManagementStore_UnbanPubkey` — `crates/core/tests/access.rs` unbanning_lifts_the_ban_without_admitting
- `TestManagementStore_UnallowPubkey` — `crates/core/tests/access.rs` membership_changes_are_idempotent_and_published
- `TestManagementStore_BanEvent` — `crates/core/tests/access.rs` banning_an_event_deletes_it_until_it_is_allowed
- `TestManagementStore_AllowEvent` — `crates/core/tests/access.rs` banning_an_event_deletes_it_until_it_is_allowed
- `TestManagementStore_BanCacheInvalidation` — `crates/relay/tests/access.rs` a_banned_member_stops_receiving_at_once
- `TestManagementStore_CreateRole` — `crates/core/tests/access.rs` roles_are_defined_published_and_assigned
- `TestManagementStore_CreateRole_Duplicate` — `crates/core/tests/access.rs` roles_are_defined_published_and_assigned
- `TestManagementStore_CreateRole_OmitsEmptyAndZero` — `crates/core/tests/access.rs` roles_are_defined_published_and_assigned
- `TestManagementStore_CreateRole_InvalidColor` — `crates/core/tests/access.rs` roles_are_defined_published_and_assigned
- `TestManagementStore_EditRole` — `crates/core/tests/access.rs` roles_are_defined_published_and_assigned
- `TestManagementStore_AssignAndUnassignRole` — `crates/core/tests/access.rs` roles_are_defined_published_and_assigned
- `TestManagementStore_AssignRole_UnknownRole` — `crates/core/tests/access.rs` roles_are_defined_published_and_assigned
- `TestManagementStore_AssignAndUnassignMethod` — `crates/core/tests/access.rs` methods_are_granted_individually_and_to_members_of_inviting_relays
- `TestManagementStore_GetMethodAssigneeItems` — `crates/core/tests/access.rs` methods_are_granted_individually_and_to_members_of_inviting_relays
- `TestManagementStore_MemberMethods` — `crates/relay/tests/access.rs` members_of_an_inviting_relay_may_create_claims
- `TestManagementStore_RemoveMember_RemovesAssignedMethods` — `crates/core/tests/access.rs` removing_a_member_takes_their_methods
- `TestManagementStore_AssignMethod_UnknownMethod` — `crates/core/tests/access.rs` method_grants_are_checked
- `TestManagementStore_MethodAssignments_OwnerAndSelf` — `crates/core/tests/access.rs` method_grants_are_checked
- `TestManagementStore_AccessChecks` — `crates/core/tests/relay_policy.rs` members_and_managers_may_write_and_strangers_may_not
- `TestManagementStore_MethodCacheInvalidation` — `crates/relay/tests/access.rs` management_calls_are_authorized_per_method
- `TestManagementStore_MemberCacheInvalidation` — `crates/relay/tests/access.rs` a_member_added_through_the_api_can_read_straight_away
- `TestManagementStore_DeleteRole` — `crates/core/tests/access.rs` deleting_a_role_takes_it_from_its_holders_and_publishes_a_deletion
- `TestManagementStore_DeleteRole_BroadcastsDeletion` — `crates/core/tests/access.rs` deleting_a_role_takes_it_from_its_holders_and_publishes_a_deletion
- `TestManagementStore_PubkeyIsBanned_NotBanned` — `crates/core/tests/relay_policy.rs` banned_authors_and_events_are_blocked
- `TestManagementStore_EventIsBanned_NotBanned` — `crates/core/tests/relay_policy.rs` banned_authors_and_events_are_blocked
- `TestManagementStore_CreateClaim` — `crates/core/tests/access.rs` claims_are_created_listed_revoked_and_logged
- `TestManagementStore_CreateClaim_Idempotent` — `crates/core/tests/access.rs` claims_are_created_listed_revoked_and_logged
- `TestManagementStore_CreateClaim_Empty` — `crates/core/tests/access.rs` claims_are_created_listed_revoked_and_logged
- `TestManagementStore_GetClaims_ReturnsAll` — `crates/core/tests/access.rs` claims_are_created_listed_revoked_and_logged
- `TestManagementStore_DeleteClaim` — `crates/core/tests/access.rs` claims_are_created_listed_revoked_and_logged
- `TestManagementStore_DeleteClaim_RemovesAllMatching` — Obsolete: a claim is one row, so there is never more than one to remove.
- `TestManagementStore_CreateClaim_ValidatesJoinRequest` — `crates/relay/tests/access.rs` a_claim_lets_a_stranger_join_and_read
- `TestManagementStore_Enable_WiresListBannedEvents` — `crates/core/tests/access.rs` banning_an_event_deletes_it_until_it_is_allowed
- `TestManagementStore_Enable_PerMethodAuthorization` — `crates/relay/tests/access.rs` management_calls_are_authorized_per_method
- `TestManagementStore_Enable_SupportedMethodsPerPubkey` — `crates/relay/tests/access.rs` management_calls_are_authorized_per_method
- `TestManagementStore_ClaimLog` — `crates/core/tests/access.rs` claims_are_created_listed_revoked_and_logged
- `TestManagementStore_ClaimLog_PredatesLogging` — Obsolete: every claim has had a log since it was created.

## migrations_test.go

- `TestRunMigrations_RunsOnceInOrder` — Obsolete: sqlx runs the migrations.
- `TestRunMigrations_FailureAbortsAndRetries` — Obsolete: sqlx runs the migrations.
- `TestRemoveConfigRoles` — Obsolete: there are no config files.
- `TestRunMigrations_FailureRollsBack` — Obsolete: sqlx runs the migrations.
- `TestImportSqlite` — Obsolete: the one-time import replaces zooid migrations.
- `TestImportSqlite_UnparseableConfigAborts` — Obsolete: the one-time import replaces zooid migrations.
- `TestImportSqlite_NoDatabase` — Obsolete: the one-time import replaces zooid migrations.

## notify_test.go

- `TestNotifyStore_Set` — `crates/core/tests/notifications.rs` a_message_is_kept_and_announced
- `TestNotifyStore_SetReplaces` — `crates/core/tests/notifications.rs` a_new_message_replaces_the_old
- `TestNotifyStore_SetRequiresAMessage` — `crates/core/tests/notifications.rs` a_message_is_required
- `TestNotifyStore_GetIsPerPubkey` — `crates/core/tests/notifications.rs` messages_belong_to_one_pubkey_on_one_relay
- `TestNotifyStore_GetSkipsAnExpiredMessage` — `crates/core/tests/notifications.rs` a_message_stops_at_its_expiry
- `TestNotifyStore_GetKeepsAnUnexpiredMessage` — `crates/core/tests/notifications.rs` a_message_stops_at_its_expiry
- `TestNotifyStore_Clear` — `crates/core/tests/notifications.rs` clearing_a_message_is_idempotent
- `TestNotifyStore_ClearIsIdempotent` — `crates/core/tests/notifications.rs` clearing_a_message_is_idempotent
- `TestNotifyStore_NotificationsAreNotServed` — Obsolete: notifications are a table, not events a relay could serve.

## push_test.go

- `TestPushManager_ValidatePushSubscription_RejectsLocalCallback` — `crates/relay/tests/push.rs` a_local_callback_is_refused
- `TestPushManager_ValidatePushSubscription_AcceptsPublicCallback` — `crates/relay/tests/push.rs` a_local_callback_is_refused
- `TestPushManager_ValidatePushSubscription_RelayTagNormalization` — `crates/relay/tests/push.rs` the_relay_tag_must_name_this_relay
- `TestPushManager_Enable_ClientRefusesToDialLoopback` — `crates/adapters/src/outbound.rs` a_name_resolving_to_loopback_is_never_dialed

## tenancy_test.go

- `TestEventStore_TenantIsolation` — `crates/core/tests/events.rs` relays_never_see_each_others_events
- `TestEventStore_ReplaceEvent_TenantIsolation` — `crates/core/tests/events.rs` a_replaceable_event_keeps_one_newest_version_per_slot
- `TestEventStore_ReplaceEvent_Concurrent` — `crates/core/tests/events.rs` racing_versions_of_a_slot_leave_the_newest
- `TestEventStore_TieOrder` — `crates/core/tests/events.rs` limits_cap_results_and_ties_break_on_id
- `TestEventStore_SaveEvent_RejectsNul` — `crates/core/tests/events.rs` events_with_a_nul_character_are_not_storable

## transfer_test.go

- `TestExportImportRoundTrip` — `crates/core/tests/transfer.rs` an_export_imports_into_another_relay
- `TestImportEventsIsIdempotent` — `crates/core/tests/transfer.rs` importing_a_dump_twice_is_harmless
- `TestImportEventsSkipsBadLines` — `crates/core/tests/transfer.rs` an_import_skips_bad_lines_and_says_which
- `TestValidateImportEvent` — `crates/core/tests/transfer.rs` an_import_skips_bad_lines_and_says_which
- `TestImportEventsRebuildsGroupState` — `crates/core/tests/transfer.rs` an_import_rebuilds_room_state
- `TestImportEventsAdmitsJoinRequests` — `crates/core/tests/transfer.rs` an_import_admits_join_requests
- `TestImportEventsAddsToAnExistingGroup` — `crates/core/tests/transfer.rs` an_import_adds_to_an_existing_room
- `TestImportEventsLeavesUncreatedGroupAlone` — `crates/core/tests/transfer.rs` an_import_leaves_an_uncreated_room_alone
- `TestImportEventsDeletesDeletedGroup` — `crates/core/tests/transfer.rs` an_import_ending_in_a_deletion_leaves_no_room
- `TestImportEventsKeepsGroupRecreatedAfterDelete` — `crates/core/tests/transfer.rs` an_import_keeps_a_room_created_again_after_its_deletion

## util_test.go

- `TestValidateNIP98Auth_AcceptsFreshEvent` — `crates/api/tests/auth.rs` a_signature_buys_a_session_until_it_is_deleted
- `TestValidateNIP98Auth_RejectsStaleEvent` — `crates/api/tests/auth.rs` a_session_signature_must_be_for_this_request_and_fresh
- `TestValidateNIP98Auth_RejectsFutureEvent` — `crates/api/tests/auth.rs` a_session_signature_must_be_for_this_request_and_fresh
- `TestIsInternalEvent_BlobDescriptor` — `crates/core/tests/relay_policy.rs` kinds_the_relay_writes_itself_are_refused
