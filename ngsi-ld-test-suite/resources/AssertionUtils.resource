*** Settings ***
Library     ${EXECDIR}/libraries/assertionUtils.py
Library     ${EXECDIR}/libraries/dateTimeUtils.py
Library     RequestsLibrary
Library     Collections
Library     JSONLibrary
Library     String
Resource    ${EXECDIR}/resources/ApiUtils/jsonldContext.resource


*** Variables ***
${id_regex_expr}=                           root\\['id'\\]
${instance_id_regex_expr}=                  root\\[.*\\]\\['instanceId'\\]
${notification_timestamps_regex_expr}=      root\\['last.*'\\]
${notification_endpoint_uri_regex_expr}=    root\\['endpoint'\\]\\['uri'\\]
${context_regex_expr}=                      root\\['@context'\\]
${status_regex_expr}=                       root\\['status'\\]
${lastfailure_regex_expr}=                  root\\['lastFailure'\\]
${lastNotification_regex_expr}=             root\\['lastNotification'\\]
${timesFailed_regex_expr}=                  root\\['timesFailed'\\]
${timesSent_regex_expr}=                    root\\['timesSent'\\]
${is_active_expr}=                          root\\['isActive'\\]
${context_source_ignored_regex_expr}=       root\\['@context'\\]


*** Keywords ***
Check Response Status Code
    [Arguments]    ${expected_status_code}    ${response_status_code}
    ${response_status_code}=    convert to string    ${response_status_code}
    Should Be Equal
    ...    ${expected_status_code}
    ...    ${response_status_code}
    ...    HTTP status code comparison failed with (expected, actual) ->

Check Response Body Containing Array Of URIs set to
    [Arguments]    ${expected_entities_ids}    ${response_body}
    Lists Should Be Equal    ${expected_entities_ids}    ${response_body}    ignore_order=True

Check Response Body Containing Entities URIS set to
    [Arguments]    ${expected_entities_ids}    ${response_body}    ${ignore_order}=True
    @{response_entities_ids}=    Create List
    FOR    ${entity}    IN    @{response_body}
        Append To List    ${response_entities_ids}    ${entity['id']}
    END

    Lists Should Be Equal    ${expected_entities_ids}    ${response_entities_ids}    ignore_order=${ignore_order}

Check Response Body Content
    [Arguments]    ${expectation_filename}    ${response_body}    ${additional_ignored_path}=${EMPTY}
    ${entity_payload}=    Load JSON From File    ${EXECDIR}/data/entities/expectations/${expectation_filename}
    IF    $additional_ignored_path!=''
        ${all_ignored_paths}=    Create List    ${instance_id_regex_expr}    ${additional_ignored_path}
    ELSE
        ${all_ignored_paths}=    Create List    ${instance_id_regex_expr}
    END
    ${comparison_result}=    Compare Dictionaries Ignoring Keys
    ...    ${entity_payload}
    ...    ${response_body}
    ...    ${all_ignored_paths}
    Should Be Empty    ${comparison_result}    msg=${comparison_result.pretty()}

Check Response Body Is Empty
    [Arguments]    ${response}
    ${response_text}=    Convert To String    ${response.text}
    Should Be Empty    ${response_text}

Check Response Headers Containing Content-Type set to
    [Arguments]    ${expected_content_type_content}    ${response_headers}
    Should Be Equal    ${response_headers['Content-Type']}    ${expected_content_type_content}

Check Response Headers Link Not Empty
    [Arguments]    ${response_headers}
    Should Not Be Empty    ${response_headers['Link']}

Check Response Headers Link set to
    [Arguments]    ${response_headers}    ${expected_link_header}
    Should Be Equal    ${response_headers['Link']}    ${expected_link_header}

# Since Http headers names are case-insensitive (from Http specification)
# We check both Location and location headers

Check Response Headers Containing URI set to
    [Arguments]    ${expected_entity_id}    ${response_headers}

    IF    'Location' in ${response_headers}
        Should Contain
        ...    ${response_headers['Location']}
        ...    ${expected_entity_id}
        ...    ignore_order=True
    END

Check Response Headers Containing NGSILD-Results-Count Equals To
    [Arguments]    ${expected_result_count}    ${response_headers}
    Should Be Equal    ${response_headers['NGSILD-Results-Count']}    ${expected_result_count}

# Since Http headers names are case-insensitive (from Http specification)
# We check both Location and location headers

Check Response Headers ID Not Empty
    [Arguments]    ${response_headers}

    ${location_header}=    Set Variable If
    ...    'Location' in ${response_headers}
    ...    ${response_headers['Location']}
    ${id}=    Fetch From Right    ${location_header}    /
    Should Not Be Empty    ${id}
    RETURN    ${id}

Check Response Body Containing an Attribute set to
    [Arguments]    ${expected_attribute_name}    ${response_body}    ${expected_attribute_value}=${EMPTY}
    Should Not Be Empty    ${response_body['${expected_attribute_name}']}
    IF    $expected_attribute_value!=''
        Should Be Equal    ${response_body['${expected_attribute_name}']}    ${expected_attribute_value}
    END

Check Response Body Containing a Boolean Attribute set to
    [Arguments]    ${expected_attribute_name}    ${response_body}    ${expected_attribute_value}
    Dictionary Should Contain Key    ${response_body}    ${expected_attribute_name}
    IF    '${expected_attribute_value}'=='True'
        Should Be True    ${response_body['${expected_attribute_name}']}
    ELSE
        IF    '${expected_attribute_value}'=='False'
            Should Not Be True    ${response_body['${expected_attribute_name}']}
        ELSE
            Fail    Unexpected boolean value, only True/False are allowed, received ${expected_attribute_value}
        END
    END

Check Response Body Containing Batch Operation Result
    [Arguments]    ${expected_batch_operation_result}    ${response_body}
    @{expected_successful_entities_ids}=    Get From Dictionary    ${expected_batch_operation_result}    success
    @{expected_failed_entities_ids}=    Get From Dictionary    ${expected_batch_operation_result}    errors
    @{response_errors}=    Get From Dictionary    ${response_body}    errors

    ${expected_failed_entities_ids_length}=    Get Length    ${expected_failed_entities_ids}
    ${response_errors_length}=    Get Length    ${response_errors}

    Lists Should Be Equal    ${expected_successful_entities_ids}    ${response_body['success']}    ignore_order=True
    Should be Equal as Integers    ${expected_failed_entities_ids_length}    ${response_errors_length}
    FOR    ${response_error}    IN    @{response_errors}
        List Should Contain Value    ${expected_failed_entities_ids}    ${response_error['entityId']}
        Should Not Be Empty    ${response_error['error']}
    END

Check Response Body Containing Update Result
    [Arguments]    ${expected_update_result}    ${response_body}
    @{expected_successful_attributes_names}=    Get From Dictionary    ${expected_update_result}    updated
    @{expected_failed_attributes_names}=    Get From Dictionary    ${expected_update_result}    notUpdated
    ${expected_failed_attributes_names_length}=    Get Length    ${expected_failed_attributes_names}

    @{response_errors}=    Get From Dictionary    ${response_body}    notUpdated
    ${response_errors_length}=    Get Length    ${response_errors}

    Lists Should Be Equal
    ...    ${expected_successful_attributes_names}
    ...    ${response_body['updated']}
    ...    ignore_order=True
    Should be Equal as Integers    ${expected_failed_attributes_names_length}    ${response_errors_length}
    FOR    ${response_error}    IN    @{response_errors}
        List Should Contain Value    ${expected_failed_attributes_names}    ${response_error['attributeName']}
        Should Not Be Empty    ${response_error['reason']}
    END

Check Response Body Containing Entity element
    [Arguments]    ${expectation_filename}    ${entity_id}    ${response_body}    ${ignore_core_context_version}=False
    ${entity_payload}=    Load JSON From File    ${EXECDIR}/data/entities/expectations/${expectation_filename}
    ${entity}=    Update Value To JSON    ${entity_payload}    $..id    ${entity_id}
    ${comparison_result}=    Compare Dictionaries Ignoring Keys
    ...    ${entity}
    ...    ${response_body}
    ...    ${instance_id_regex_expr}
    ...    ${ignore_core_context_version}
    Should Be Empty    ${comparison_result}    msg=${comparison_result.pretty()}

Check Response Body Containing List Containing Entity Elements
    [Arguments]
    ...    ${expectation_filename}
    ...    ${entities_ids}
    ...    ${response_body}
    ...    ${ignore_core_context_version}=False
    FOR    ${entity_id}    IN    @{entities_ids}
        ${entity}=    Get Value From JSON    ${response_body}    $[?(@.id=='${entity_id}')]
        Check Response Body Containing Entity element
        ...    ${expectation_filename}
        ...    ${entity_id}
        ...    ${entity}[0]
        ...    ${ignore_core_context_version}
    END

Check Response Body Containing List Containing Entity Elements With Different Types
    [Arguments]
    ...    ${filename}
    ...    ${entities_representation_ids}
    ...    ${response_body}
    ...    ${ignore_core_context_version}=False
    ${entities_representation_payload}=    Load JSON From File    ${EXECDIR}/data/entities/expectations/${filename}
    ${index}=    Set Variable    0
    FOR    ${entity_representation_id}    IN    @{entities_representation_ids}
        ${entities_representation_payload}=    Update Value To JSON
        ...    ${entities_representation_payload}
        ...    $.[${index}]..id
        ...    ${entity_representation_id}
        ${index}=    Evaluate    ${index} + 1
    END
    ${comparison_result}=    Compare Dictionaries Ignoring Keys
    ...    ${entities_representation_payload}
    ...    ${response_body}
    ...    ${instance_id_regex_expr}
    ...    ${ignore_core_context_version}
    ...    group_by=id
    Should Be Empty    ${comparison_result}    msg=${comparison_result.pretty()}

Check Response Body Containing EntityTemporal element
    [Arguments]
    ...    ${filename}
    ...    ${temporal_entity_representation_id}
    ...    ${response_body}
    ...    ${excluded_regex}=${EMPTY}
    ${temporal_entity_representation_payload}=    Load JSON From File
    ...    ${EXECDIR}/data/temporalEntities/expectations/${filename}
    ${all_excluded_regex}=    Create List    ${instance_id_regex_expr}
    IF    $excluded_regex != ''
        Append To List    ${all_excluded_regex}    ${excluded_regex}
    END
    ${temporal_entity_representation}=    Update Value To JSON
    ...    ${temporal_entity_representation_payload}
    ...    $.id
    ...    ${temporal_entity_representation_id}
    ${comparison_result}=    Compare Dictionaries Ignoring Keys
    ...    ${temporal_entity_representation}
    ...    ${response_body}
    ...    ${all_excluded_regex}
    Should Be Empty    ${comparison_result}    msg=${comparison_result.pretty()}

Check Response Body Containing List Containing EntityTemporal elements
    [Arguments]    ${filename}    ${response_body}    ${group_by}=id
    ${temporal_entities_representation_payload}=    Load JSON From File
    ...    ${EXECDIR}/data/temporalEntities/expectations/${filename}
    ${comparison_result}=    Compare Dictionaries Ignoring Keys
    ...    ${temporal_entities_representation_payload}
    ...    ${response_body}
    ...    ${instance_id_regex_expr}
    ...    group_by=${group_by}
    Should Be Empty    ${comparison_result}    msg=${comparison_result.pretty()}

Check Response Body Containing Subscription element
    [Arguments]    ${expectation_filename}    ${subscription_id}    ${response_body}    ${additional_ignored_keys}=None
    ${subscription_payload}=    Load JSON From File    ${EXECDIR}/data/${expectation_filename}
    ${subscription}=    Update Value To JSON    ${subscription_payload}    $..id    ${subscription_id}
    ${ignored_keys}=    Create List    ${context_regex_expr}
    IF    ${additional_ignored_keys} != None
        ${ignored_keys}=    Combine Lists    ${ignored_keys}    ${additional_ignored_keys}
    END
    ${comparison_result}=    Compare Dictionaries Ignoring Keys
    ...    ${subscription}
    ...    ${response_body}
    ...    ${ignored_keys}
    Should Be Empty    ${comparison_result}    msg=${comparison_result.pretty()}

Check Response Body Containing List Containing Subscription elements
    [Arguments]    ${expectation_file_path}    ${subscription_ids}    ${response_body}
    ${subscription_payload}=    Load JSON From File    ${EXECDIR}/data/${expectation_file_path}
    ${index}=    Set Variable    0
    FOR    ${subscription_id}    IN    @{subscription_ids}
        ${subscription_payload}=    Update Value To JSON
        ...    ${subscription_payload}
        ...    $.[${index}]..id
        ...    ${subscription_id}
        ${index}=    Evaluate    ${index} + 1
    END
    # exclude_regex_paths matches DeepDiff paths like root[0]['jsonldContext'] (single quotes), so the
    # ignore regexes must be bare key names. The double-quoted forms below never matched, which only
    # surfaced once the broker started emitting the spec-mandated jsonldContext member (NGSI-LD 5.2.12).
    ${ignored_keys}=    Create List
    ...    ${context_regex_expr}
    ...    jsonldContext
    ...    timesFailed
    ...    timesSent
    ...    notificationTrigger
    ${comparison_result}=    Compare Dictionaries Ignoring Keys
    ...    ${subscription_payload}
    ...    ${response_body}
    ...    ${ignored_keys}
    Should Be Empty    ${comparison_result}    msg=${comparison_result.pretty()}

# Since response body can be a json object if it contains one element
# A check on the response body type is needed

Check Response Body Containing Number Of Entities
    [Arguments]    ${expected_entity_type}    ${expected_length}    ${response_body}

    ${response_body_length}=    Get Length    ${response_body}
    ${is_list}=    Run Keyword
    ...    Evaluate    type(${response_body})==list

    IF    ${is_list}
        Should Be Equal    ${response_body_length}    ${expected_length}
    ELSE
        Should Be Equal    ${1}    ${expected_length}
    END

    FOR    ${index}    IN RANGE    ${expected_length}
        IF    ${is_list}
            Should Be Equal    ${response_body[${index}]['type']}    ${expected_entity_type}
        END
    END
    IF    ${is_list} is ${FALSE}
        Should Be Equal    ${response_body['type']}    ${expected_entity_type}
    END

Check Response Body Containing Context Source Registration element
    [Arguments]    ${expectation_filename}    ${context_source_registration_id}    ${response_body}
    ${expectation_json}=    Load JSON From File    ${EXECDIR}/data/${expectation_filename}

    ${id_dict}=    Create Dictionary    id=${context_source_registration_id}
    ${expectation_json_with_id}=    Add Object To JSON    ${expectation_json}    $    ${id_dict}

    ${ignored_keys}=    Create List    ${context_regex_expr}    mode    operations
    ${comparison_result}=    Compare Dictionaries Ignoring Keys
    ...    ${expectation_json_with_id}
    ...    ${response_body}
    ...    ${ignored_keys}
    Should Be Empty    ${comparison_result}    msg=${comparison_result.pretty()}

Check Response Body Containing EntityTypeList element
    [Arguments]    ${expectation_filename}    ${response_body}
    ${entity_type_list_payload}=    Load JSON From File    ${EXECDIR}/data/${expectation_filename}
    ${ignored_keys}=    Create List    ${id_regex_expr}
    ${comparison_result}=    Compare Dictionaries Ignoring Keys
    ...    ${entity_type_list_payload}
    ...    ${response_body}
    ...    ${ignored_keys}
    Should Be Empty    ${comparison_result}    msg=${comparison_result.pretty()}

Check Response Body Containing EntityType element
    [Arguments]    ${expectation_filename}    ${response_body}
    ${entity_type_payload}=    Load JSON From File    ${EXECDIR}/data/${expectation_filename}
    ${ignored_keys}=    Create List
    ${comparison_result}=    Compare Dictionaries Ignoring Keys
    ...    ${entity_type_payload}
    ...    ${response_body}
    ...    ${ignored_keys}
    Should Be Empty    ${comparison_result}    msg=${comparison_result.pretty()}

Check Response Body Containing EntityTypeInfo element
    [Arguments]    ${expectation_filename}    ${response_body}
    ${entity_type_info_payload}=    Load JSON From File    ${EXECDIR}/data/${expectation_filename}
    ${ignored_keys}=    Create List
    ${comparison_result}=    Compare Dictionaries Ignoring Keys
    ...    ${entity_type_info_payload}
    ...    ${response_body}
    ...    ${ignored_keys}
    Should Be Empty    ${comparison_result}    msg=${comparison_result.pretty()}

Check Response Body Containing AttributeList element
    [Arguments]    ${expectation_filename}    ${response_body}
    ${attribute_list_payload}=    Load JSON From File    ${EXECDIR}/data/${expectation_filename}
    ${ignored_keys}=    Create List    ${id_regex_expr}
    ${comparison_result}=    Compare Dictionaries Ignoring Keys
    ...    ${attribute_list_payload}
    ...    ${response_body}
    ...    ${ignored_keys}
    Should Be Empty    ${comparison_result}    msg=${comparison_result.pretty()}

Check Response Body Containing Attribute element
    [Arguments]    ${expectation_filename}    ${response_body}
    ${attribute_payload}=    Load JSON From File    ${EXECDIR}/data/${expectation_filename}
    ${ignored_keys}=    Create List
    ${comparison_result}=    Compare Dictionaries Ignoring Keys
    ...    ${attribute_payload}
    ...    ${response_body}
    ...    ${ignored_keys}
    Should Be Empty    ${comparison_result}    msg=${comparison_result.pretty()}

Check Response Body Containing List Containing Context Source Registrations elements
    [Arguments]
    ...    ${expectation_file_path}
    ...    ${expected_context_source_registrations_ids}
    ...    ${response_body}
    ...    ${ignore_default}=${True}
    ${expected_context_source_registrations_payload}=    Load JSON From File
    ...    ${EXECDIR}/data/${expectation_file_path}
    ${index}=    Set Variable    0
    FOR    ${expected_context_source_registration_id}    IN    @{expected_context_source_registrations_ids}
        ${expected_context_source_registrations_payload}=    Update Value To JSON
        ...    ${expected_context_source_registrations_payload}
        ...    $.[${index}].id
        ...    ${expected_context_source_registration_id}
        ${index}=    Evaluate    ${index} + 1
    END

    ${ignored_attributes}=    Create List    ${status_regex_expr}

    IF    ${ignore_default}
        Append To List    ${ignored_attributes}    mode    operations
    END

    ${comparison_result}=    Compare Dictionaries Ignoring Keys
    ...    ${expected_context_source_registrations_payload}
    ...    ${response_body}
    ...    ${ignored_attributes}
    ...    group_by=id
    Should Be Empty    ${comparison_result}    msg=${comparison_result.pretty()}

Check Response Body Containing ProblemDetails Element Containing Type Element set to
    [Arguments]    ${response_body}    ${type}
    Should Be Equal    ${response_body['type']}    ${type}

Check Response Body Containing ProblemDetails Element Containing Title Element
    [Arguments]    ${response_body}
    Should Not Be Empty    ${response_body['title']}

Check Response Body Containing ProblemDetails Element
    [Arguments]    ${response_body}    ${problem_type}
    Should Not Be Empty    ${response_body['title']}
    Should Not Be Empty    ${response_body['detail']}
    Should Be String    ${response_body['title']}
    Should Be String    ${response_body['detail']}
    Should Not Be Equal    ${response_body['title']}    ${response_body['detail']}
    Should Be Equal    ${response_body['type']}    ${problem_type}

Check Response Does Not Contain Body
    [Arguments]    ${response}
    Should Not Contain    ${response}    body

Check Response Reason set to
    [Arguments]    ${response_reason}    ${reason}
    Should Be Equal    ${response_reason}    ${reason}

Check Response Kind set to
    [Arguments]    ${response}    ${kind}
    Should Be Equal    ${response['kind']}    ${kind}

Check JSON Value In Response Body
    [Arguments]    ${json_path_expr}    ${value_to_check}    ${response_body}
    Should Be Equal As Strings    ${response_body${json_path_expr}}    ${value_to_check}

Check JSON Value Not In Response Body
    [Arguments]    ${json_path_expr}    ${response_body}
    Should Not Have Value In Json    ${response_body}    ${json_path_expr}

Check NotificationParams
    [Arguments]    ${filename}    ${expected_additional_members}    ${response_body}

    ${expected_notification}=    Load JSON From File    ${EXECDIR}/data/${filename}
    ${ignored_keys}=    Create List    ${notification_timestamps_regex_expr}    ${notification_endpoint_uri_regex_expr}
    ${comparison_result}=    Compare Dictionaries Ignoring Keys
    ...    ${expected_notification}
    ...    ${response_body['notification']}
    ...    ${ignored_keys}
    Should Be Empty    ${comparison_result}    msg=${comparison_result.pretty()}

    FOR    ${expected_additional_member}    IN    @{expected_additional_members}
        Should Not Be Empty    ${response_body['notification']['${expected_additional_member}']}
    END

Check Notification Containing Entity Element
    [Arguments]    ${filename}    ${notification}
    ${expected_entity_in_notification}=    Load JSON From File
    ...    ${EXECDIR}/data/subscriptions/expectations/${filename}
    ${ignored_keys}=    Create List    id
    ${comparison_result}=    Compare Dictionaries Ignoring Keys
    ...    ${expected_entity_in_notification}
    ...    ${notification['data'][0]}
    ...    ${ignored_keys}
    Should Be Empty    ${comparison_result}    msg=${comparison_result.pretty()}

Check Notification Containing Entities Elements
    [Arguments]    ${filename}    ${notification}
    ${expected_entities_in_notification}=    Load JSON From File
    ...    ${EXECDIR}/data/subscriptions/expectations/${filename}
    ${ignored_keys}=    Create List    id
    ${comparison_result}=    Compare Dictionaries Ignoring Keys
    ...    expected=${expected_entities_in_notification}
    ...    actual=${notification['data']}
    ...    exclude_regex_paths=${ignored_keys}
    ...    group_by=id
    Should Be Empty    ${comparison_result}    msg=${comparison_result.pretty()}

Check Pagination Prev And Next Headers
    [Arguments]    ${prev_link}    ${next_link}    ${response_headers}
    # When both links are empty no pagination is expected and the Link header may be absent; skip.
    IF    '${prev_link}'=='' and '${next_link}'==''    RETURN
    # NGSI-LD 6.3.5: a application/json response also conveys the @context as a Link header value.
    # Pagination assertions must consider only the rel="prev"/rel="next" links and ignore the context link.
    ${actual_links}=    Evaluate
    ...    [l for l in $response_headers['Link'].replace(" ", "").split(',') if 'json-ld#context' not in l]
    ${expected_links}=    Create List
    IF    '${prev_link}'!=''    Append To List    ${expected_links}    ${prev_link.replace(" ", "")}
    IF    '${next_link}'!=''    Append To List    ${expected_links}    ${next_link.replace(" ", "")}
    Lists Should Be Equal    ${actual_links}    ${expected_links}    ignore_order=True

Check Resource Set To
    [Arguments]    ${expected_resource}    ${response_body}    ${ignored_keys}=${None}    ${group_by}=${None}

    ${comparison_result}=    Compare Dictionaries Ignoring Keys
    ...    ${expected_resource}
    ...    ${response_body}
    ...    ${ignored_keys}
    ...    group_by=${group_by}
    Should Be Empty    ${comparison_result}    msg=${comparison_result.pretty()}

Check Created Resource Set To
    [Arguments]    ${created_resource}    ${response_body}    ${ignored_keys}=${None}

    Check Resource Set To    ${created_resource}    ${response_body}    ${ignored_keys}

Check Created Resources Set To
    [Arguments]    ${expected_resources}    ${response_body}    ${ignored_keys}=${None}

    Check Resource Set To    ${expected_resources}    ${response_body}    ${ignored_keys}    group_by=id

Check Updated Resource Set To
    [Arguments]    ${updated_resource}    ${response_body}    ${ignored_keys}=${None}

    Check Resource Set To    ${updated_resource}    ${response_body}    ${ignored_keys}

Check Updated Resources Set To
    [Arguments]    ${updated_resources}    ${response_body}    ${ignored_keys}=${None}

    Check Resource Set To    ${updated_resources}    ${response_body}    ${ignored_keys}    group_by=id

Check SUT Not Containing Resource
    [Arguments]    ${response_status}
    ${response_status_string}=    convert to string    ${response_status}
    Should Be Equal    ${response_status_string}    404

Check SUT Not Containing Resources
    [Arguments]    ${response_body}
    Should Be Empty    ${response_body}

Check Response Body Might Contain Additional Members of the NotificationParams
    # Check the Additional Members 5.2.14.2 in the NotificationParams data structure
    [Arguments]    ${dictionary}    ${key}
    Log    Check Response Body Might Contain Additional Members of the NotificationParams
    Log    Response Body: ${dictionary}
    Log    Key: ${key}

    ${value}=    Evaluate    $dictionary.get($key)

    IF    '${value}' != '' and '${value}' != 'None'
        IF    "${key}" == "lastNotification" or "${key}" == "lastFailure" or "${key}" == "lastSuccess"
            # Need to check that there is a DateTime value
            ${type date}=    Parse Ngsild Date    ${dictionary}[${key}]
            Should Not Be Equal
            ...    ${type date}
            ...    ${None}
            ...    The Additional Member ${key} contains a value (${value}) that it is not in DateTime format
        END
        IF    "${key}" == "timesSent"
            # Need to check that there is a Integer value
            ${type int}=    Evaluate    type(${dictionary}[${key}]).__name__
            Should Be Equal
            ...    ${type int}
            ...    int
            ...    The Additonal Member ${key} contains a value (${value}) that it is not in Number format
        END
    ELSE
        Log    The key '${key}' is not present in the dictionary
    END

Check Response Body Might Contain Optional Fields
    [Arguments]    ${dictionary}    ${key}
    Log    Check Response Body Might Contain Optional Fields
    Log    Response Body: ${dictionary}
    Log    Key: ${key}

    ${value}=    Evaluate    $dictionary.get($key)

    IF    '${value}' != '' and '${value}' != 'None'
        IF    "${key}" == "expiresAt" or "${key}" == "lastUsage"
            # Need to check that there is a DateTime value
            ${type date}=    Parse Ngsild Date    ${dictionary}[${key}]
            Should Not Be Equal
            ...    ${type date}
            ...    ${None}
            ...    The Additional Member ${key} contains a value (${value}) that it is not in DateTime format
        END
        IF    "${key}" == "throttling" or "${key}" == "timeInterval" or "${key}" == "numberOfHits"
            # Need to check that there is a Integer value
            ${type int}=    Evaluate    type(${dictionary}[${key}]).__name__
            Should Be Equal
            ...    ${type int}
            ...    int
            ...    The Additonal Member ${key} contains a value (${value}) that it is not in Number format
        END
        IF    "${key}" == "isActive"
            # Need to check that there is a Boolean balue
            ${type bool}=    Evaluate    type(${dictionary}[${key}]).__name__
            Should Be Equal
            ...    ${type bool}
            ...    bool
            ...    The Additonal Member ${key} contains a value (${value}) that it is not in Boolean format
        END
        IF    "${key}" == "subscriptionName" or "${key}" == "description" or "${key}" == "q" or "${key}" == "csf" or "${key}" == "status"
            # Need to check that there is a Boolean balue
            ${type string}=    Evaluate    type(${dictionary}[${key}]).__name__
            Should Be Equal
            ...    ${type string}
            ...    str
            ...    The Additonal Member ${key} contains a value (${value}) that it is not in String format
        END
        IF    "${key}" == "extraInfo"    Log    Found "extraInfo" key
    ELSE
        Log    The key '${key}' is not present in the dictionary
    END

Check Response Body Contains DateTime Value
    [Arguments]    ${dictionary}    ${key}    ${expected value}
    Dictionary Should Contain Key    ${dictionary}    ${key}

    ${obtained value}=    Evaluate    $dictionary.get($key)
    ${date}=    Parse Ngsild Date    ${dictionary}[${key}]

    Should Not Be Equal
    ...    ${date}
    ...    ${None}
    ...    The key (${key}) contains a value (${obtained value}) that it is not in DateTime format

    Should Be Equal
    ...    ${expected value}
    ...    ${obtained value}
    ...    The expected value (${expected value}) is not the same as the obtained value (${obtained value})

Check Context Response Body Content
    [Arguments]    ${expectation_filename}    ${response_body}

    ${context_payload}=    Load JSON From File    ${EXECDIR}/data/jsonldContext/${expectation_filename}
    Dictionaries Should Be Equal    ${response_body}    ${context_payload}

Check Context Response Body Containing Detailed Information
    [Arguments]    ${response_body}    ${context_type}

    Should Not Be Empty    ${response_body['URL']}
    Should Not Be Empty    ${response_body['localId']}
    Should Not Be Empty    ${response_body['kind']}

    Should Be String    ${response_body['URL']}
    Should Be String    ${response_body['localId']}
    Should Be String    ${response_body['kind']}

    IF    $response_body['kind'] not in $ALLOWED_CONTEXT
        fail    The value of kind:"${response_body['kind']}" is not allowed
    END

    Should Be Equal    ${response_body['kind']}    ${context_type}

    Should Not Be Empty    ${response_body['createdAt']}
    ${date}=    Parse Ngsild Date    ${response_body['createdAt']}
    Should Not Be Equal
    ...    ${date}
    ...    ${None}
    ...    The createdAt key contains a value (${response_body['createdAt']}) that it is not in DateTime format

Check Context Response Body Containing numberOfHits value
    [Arguments]    ${response_body}    ${expected_number_of_hists}

    ${value}=    Evaluate    $response_body.get('numberOfHits')

    IF    '${value}' != '' and '${value}' != 'None'
        Log    Expected number of hits: ${expected_number_of_hists}
        Log    Received number of hits: ${response_body['numberOfHits']}

        Should Be Equal As Numbers    ${response_body['numberOfHits']}    ${expected_number_of_hists}
    END

Check Context Response Kind
    [Arguments]    ${response_body}    ${context_type}

    Should Be Equal    ${response_body['kind']}    ${context_type}

Check Context Response Body Containing a list of identifiers
    [Arguments]    ${response_body}    ${list_contexts}=${EMPTY}    ${kind}=${EMPTY}    ${details}=${FALSE}
    ${found}=    Set Variable    ${TRUE}
    IF    ${details}
        FOR    ${index}    ${uri_context}    IN ENUMERATE    @{list_contexts}
            ${found}=    Set Variable    ${FALSE}
            FOR    ${index2}    ${respObj}    IN ENUMERATE    @{response_body}
                ${uri}=    Evaluate    urllib.parse.unquote("${respObj['URL']}")    modules=urllib
                ${entryFound}=    Run Keyword And Return Status
                ...    Should End With    ${uri}    ${uri_context}
                IF    ${entryFound}
                    IF    '${kind}' != ''
                        Check Context Response Body Containing Detailed Information    ${respObj}    ${kind}
                        Check Response Body Might Contain Optional Fields    ${respObj}    lastUsage
                        Check Response Body Might Contain Optional Fields    ${respObj}    numberOfHits
                        Check Response Body Might Contain Optional Fields    ${respObj}    extraInfo
                    END
                    ${found}=    Set Variable    ${TRUE}
                    BREAK
                END
            END
            Should Be True    ${found}
        END
    ELSE
        FOR    ${index}    ${uri_context}    IN ENUMERATE    @{list_contexts}
            ${found}=    Set Variable    ${FALSE}
            FOR    ${index2}    ${respObj}    IN ENUMERATE    @{response_body}
                ${uri}=    Evaluate    urllib.parse.unquote("${respObj}")    modules=urllib
                ${entryFound}=    Run Keyword And Return Status
                ...    Should End With    ${uri}    ${uri_context}
                IF    ${entryFound}
                    ${found}=    Set Variable    ${TRUE}
                    BREAK
                END
            END
            Should Be True    ${found}
        END
        Should Be True    ${found}
    END

Check Cached @Contexts
    [Arguments]    ${filename}

    ${entity_payload}=    Load JSON From File    ${EXECDIR}/data/jsonldContext/${filename}
    ${contexts}=    Get From Dictionary    ${entity_payload}    @context

    &{params}=    Create Dictionary
    Set To Dictionary    ${params}    details=true

    FOR    ${contextId}    IN    @{contexts}
        ${response}=    Serve a @context    ${contextId}    true
        Check Response Kind set to    ${response.json()}    Cached
    END

Check Context Detailed Information Keys
    [Arguments]    ${my_dict}

    ${expected_keys}=    Create List
    ...    URL
    ...    localId
    ...    kind
    ...    createdAt
    ...    lastUsage
    ...    numberOfHits
    ...    extraInfo
    ...    body
    ...    expiresAt
    ${actual_keys}=    Get Dictionary Keys    ${my_dict}

    FOR    ${key}    IN    @{expected_keys}
        Remove Values From List    ${actual_keys}    ${key}
    END

    Should Be Empty
    ...    ${actual_keys}
    ...    msg=The @context detailed information contains keys not expected: ${actual_keys}

Check Context Response Body Containing a JSONObject with details of a ImplicitlyCreated @contexts
    [Arguments]    ${response}    ${expected_length}

    ${obtained_length}=    Get Length    ${response}
    Length Should Be
    ...    ${response}
    ...    ${expected_length}
    ...    msg=The expected number of @contexts was ${expected_length} but we have obtained ${obtained_length}

    # Check mandatory keys in the response (URL, localId, kind, timestamp) and their possible values
    Check Context Response Body Containing Detailed Information    ${response}[0]    ImplicitlyCreated

    # Check optional keys in the response (lastUsage, numberOfHits, extraInfo) and their possible values
    Check Response Body Might Contain Optional Fields    ${response}[0]    lastUsage
    Check Response Body Might Contain Optional Fields    ${response}[0]    numberOfHits
    Check Response Body Might Contain Optional Fields    ${response}[0]    extraInfo

    # Check that there is no other keys
    Check Context Detailed Information Keys    ${response}[0]

Check Content Range Part Equal
    [Arguments]    ${expectedPart}    ${part}
    Should Be Equal As Strings    ${expectedPart}    ${part}

Check Data Is Empty
    [Arguments]    ${data}
    Should Be Empty    ${data}

Check Messages Contain One Instance
    [Arguments]    ${messages}
    Length Should Be    ${messages}    1

Check Message Contain Key
    [Arguments]    ${message}    ${key}
    Dictionary Should Contain Key    ${message}    ${key}

Check Message Field Equal
    [Arguments]    ${expected_field}    ${field}
    Should Be Equal As Strings    ${expected_field}    ${field}

Check Body With Alternatives
    [Arguments]    ${response_body}    ${alternatives}
    ${result}=    Set Variable    ${False}
    FOR    ${alternative}    IN    @{alternatives}
        ${result}=    Run Keyword And Return Status
        ...    Check Response Body Content    expectation_filename=${alternative}    response_body=${response_body}
        IF    ${result}    BREAK
    END
    Should Be True    ${result}
