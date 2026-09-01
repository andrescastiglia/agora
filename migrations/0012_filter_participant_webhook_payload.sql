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
    filtered_value jsonb;
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
        entry_result := jsonb_strip_nulls(jsonb_build_object('id', entry_item->'id'));
        IF jsonb_typeof(entry_item->'changes') = 'array' THEN
            filtered_changes := '[]'::jsonb;
            FOR change_item IN
                SELECT value FROM jsonb_array_elements(entry_item->'changes')
            LOOP
                change_result := change_item;
                filtered_value := '{}'::jsonb;

                IF change_result #> '{value,metadata}' IS NOT NULL THEN
                    filtered_value := filtered_value || jsonb_build_object(
                        'metadata',
                        jsonb_strip_nulls(jsonb_build_object(
                            'display_phone_number',
                            change_result #> '{value,metadata,display_phone_number}',
                            'phone_number_id',
                            change_result #> '{value,metadata,phone_number_id}'
                        ))
                    );
                END IF;

                IF jsonb_typeof(change_result #> '{value,messages}') = 'array' THEN
                    SELECT COALESCE(
                        jsonb_agg(
                            jsonb_strip_nulls(jsonb_build_object(
                                'from', item->'from',
                                'group_id', item->'group_id',
                                'id', item->'id',
                                'timestamp', item->'timestamp',
                                'type', item->'type',
                                'text', CASE
                                    WHEN jsonb_typeof(item->'text') = 'object' THEN
                                        jsonb_strip_nulls(jsonb_build_object(
                                            'body', item #> '{text,body}'
                                        ))
                                    ELSE NULL
                                END,
                                'document', CASE
                                    WHEN jsonb_typeof(item->'document') = 'object' THEN
                                        jsonb_strip_nulls(jsonb_build_object(
                                            'id', item #> '{document,id}',
                                            'filename', item #> '{document,filename}',
                                            'mime_type', item #> '{document,mime_type}',
                                            'sha256', item #> '{document,sha256}',
                                            'caption', item #> '{document,caption}'
                                        ))
                                    ELSE NULL
                                END,
                                'context', CASE
                                    WHEN item #> '{context,id}' IS NOT NULL THEN
                                        jsonb_build_object('id', item #> '{context,id}')
                                    ELSE NULL
                                END
                            ))
                            ORDER BY position
                        ),
                        '[]'::jsonb
                    )
                    INTO filtered_items
                    FROM jsonb_array_elements(change_result #> '{value,messages}')
                         WITH ORDINALITY AS items(item, position)
                    WHERE COALESCE(item->>'from' = participant_id, false) = keep_matching;
                    filtered_value := jsonb_set(
                        filtered_value,
                        '{messages}',
                        filtered_items,
                        true
                    );
                END IF;

                IF jsonb_typeof(change_result #> '{value,contacts}') = 'array' THEN
                    SELECT COALESCE(
                        jsonb_agg(
                            jsonb_strip_nulls(jsonb_build_object(
                                'wa_id', item->'wa_id',
                                'profile', CASE
                                    WHEN item #> '{profile,name}' IS NOT NULL THEN
                                        jsonb_build_object('name', item #> '{profile,name}')
                                    ELSE NULL
                                END
                            ))
                            ORDER BY position
                        ),
                        '[]'::jsonb
                    )
                    INTO filtered_items
                    FROM jsonb_array_elements(change_result #> '{value,contacts}')
                         WITH ORDINALITY AS items(item, position)
                    WHERE COALESCE(item->>'wa_id' = participant_id, false) = keep_matching;
                    filtered_value := jsonb_set(
                        filtered_value,
                        '{contacts}',
                        filtered_items,
                        true
                    );
                END IF;

                IF jsonb_typeof(change_result #> '{value,statuses}') = 'array' THEN
                    SELECT COALESCE(
                        jsonb_agg(
                            jsonb_strip_nulls(jsonb_build_object(
                                'id', item->'id',
                                'status', item->'status',
                                'timestamp', item->'timestamp',
                                'recipient_id', item->'recipient_id',
                                'recipient_type', item->'recipient_type'
                            ))
                            ORDER BY position
                        ),
                        '[]'::jsonb
                    )
                    INTO filtered_items
                    FROM jsonb_array_elements(change_result #> '{value,statuses}')
                         WITH ORDINALITY AS items(item, position)
                    WHERE COALESCE(item->>'recipient_id' = participant_id, false) = keep_matching;
                    filtered_value := jsonb_set(
                        filtered_value,
                        '{statuses}',
                        filtered_items,
                        true
                    );
                END IF;

                change_result := jsonb_strip_nulls(jsonb_build_object(
                    'field', change_item->'field',
                    'value', filtered_value
                ));

                filtered_changes := filtered_changes || jsonb_build_array(change_result);
            END LOOP;
            entry_result := jsonb_set(entry_result, '{changes}', filtered_changes, true);
        END IF;
        filtered_entries := filtered_entries || jsonb_build_array(entry_result);
    END LOOP;

    RETURN jsonb_strip_nulls(jsonb_build_object(
        'object', source_payload->'object',
        'entry', filtered_entries
    ));
END;
$$;
