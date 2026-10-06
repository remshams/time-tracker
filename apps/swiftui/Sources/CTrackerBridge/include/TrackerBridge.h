#ifndef TRACKER_BRIDGE_H
#define TRACKER_BRIDGE_H

#include <stdbool.h>

typedef struct Bridge Bridge;

Bridge *tt_bridge_open(char **error);
Bridge *tt_bridge_open_remote(const char *endpoint, char **error);
void tt_bridge_close(Bridge *bridge);
void tt_bridge_string_free(char *value);
char *tt_bridge_snapshot(Bridge *bridge, bool refresh);
char *tt_bridge_report(Bridge *bridge, const char *start, const char *end, const char *now);
char *tt_bridge_create_task_at(Bridge *bridge, const char *name, const char *occurred_at);
char *tt_bridge_rename_task_at(Bridge *bridge, const char *task_id, const char *name, const char *occurred_at);
char *tt_bridge_start_tracking(Bridge *bridge, const char *task_id);
char *tt_bridge_stop_tracking(Bridge *bridge, const char *worklog_id);
char *tt_bridge_start_tracking_at(Bridge *bridge, const char *task_id, const char *occurred_at);
char *tt_bridge_start_tracking_if_active_at(Bridge *bridge, const char *task_id, const char *expected_active_id, const char *occurred_at);
char *tt_bridge_stop_tracking_at(Bridge *bridge, const char *worklog_id, const char *occurred_at);
char *tt_bridge_pause_tracking_at(Bridge *bridge, const char *worklog_id, const char *occurred_at);
char *tt_bridge_resume_tracking_at(Bridge *bridge, const char *task_id, const char *occurred_at);
char *tt_bridge_correct_worklog_at(Bridge *bridge, const char *worklog_id, const char *expected_start, const char *expected_end, const char *replacement_start, const char *replacement_end, const char *occurred_at);
char *tt_bridge_history(Bridge *bridge, const char *task_id, const char *cursor);

#endif
