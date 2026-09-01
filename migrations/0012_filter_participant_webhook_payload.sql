CREATE OR REPLACE FUNCTION agora_filter_whatsapp_participant_payload(
    source_payload jsonb,
    participant_id text,
    keep_matching boolean
)
RETURNS jsonb
LANGUAGE plpgsql
IMMUTABLE
STRICT
PARALLEL SAFE
AS $$
DECLARE
    entry_item jsonb;
    entry_result jsonb;
    change_item jsonb;
    change_result jsonb;
    filtered_items jsonb;
    filtered_entries jsonb := '[]'::jsonb;
    filtered_changes jsonb;
BEGIN
    IF jsonb_typeof(source_payload->'entry') IS DISTINCT FROM 'array' THEN
        RETURN source_payload;
    END IF;

    FOR entry_item IN
        SELECT value FROM jsonb_array_elements(source_payload->'entry')
    LOOP
        entry_result := entry_item;
        IF jsonb_typeof(entry_item->'changes') = 'array' THEN
            filtered_changes := '[]'::jsonb;
            FOR change_item IN
                SELECT value FROM jsonb_array_elements(entry_item->'changes')
            LOOP
                change_result := change_item;

                IF jsonb_typeof(change_result #> '{value,messages}') = 'array' THEN
                    SELECT COALESCE(jsonb_agg(item ORDER BY position), '[]'::jsonb)
                    INTO filtered_items
                    FROM jsonb_array_elements(change_result #> '{value,messages}')
                         WITH ORDINALITY AS items(item, position)
                    WHERE COALESCE(item->>'from' = participant_id, false) = keep_matching;
                    change_result := jsonb_set(
                        change_result,
                        '{value,messages}',
                        filtered_items,
                        false
                    );
                END IF;

                IF jsonb_typeof(change_result #> '{value,contacts}') = 'array' THEN
                    SELECT COALESCE(jsonb_agg(item ORDER BY position), '[]'::jsonb)
                    INTO filtered_items
                    FROM jsonb_array_elements(change_result #> '{value,contacts}')
                         WITH ORDINALITY AS items(item, position)
                    WHERE COALESCE(item->>'wa_id' = participant_id, false) = keep_matching;
                    change_result := jsonb_set(
                        change_result,
                        '{value,contacts}',
                        filtered_items,
                        false
                    );
                END IF;

                IF jsonb_typeof(change_result #> '{value,statuses}') = 'array' THEN
                    SELECT COALESCE(jsonb_agg(item ORDER BY position), '[]'::jsonb)
                    INTO filtered_items
                    FROM jsonb_array_elements(change_result #> '{value,statuses}')
                         WITH ORDINALITY AS items(item, position)
                    WHERE COALESCE(item->>'recipient_id' = participant_id, false) = keep_matching;
                    change_result := jsonb_set(
                        change_result,
                        '{value,statuses}',
                        filtered_items,
                        false
                    );
                END IF;

                filtered_changes := filtered_changes || jsonb_build_array(change_result);
            END LOOP;
            entry_result := jsonb_set(entry_result, '{changes}', filtered_changes, false);
        END IF;
        filtered_entries := filtered_entries || jsonb_build_array(entry_result);
    END LOOP;

    RETURN jsonb_set(source_payload, '{entry}', filtered_entries, false);
END;
$$;
