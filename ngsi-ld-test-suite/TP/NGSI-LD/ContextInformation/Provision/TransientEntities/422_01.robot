*** Settings ***
Documentation       4.22 Transient storage of Entities and Attributes (CIM 009 V1.9.1).
...                 Antares extension TPs — 4.22 has no official ETSI TPs yet, so this
...                 suite supplies them: resources carrying a future expiresAt are
...                 served until that instant and answer 404 (entities) / vanish
...                 (attribute instances) after it, whatever operation inserted them.
...                 One shared expiry keeps the whole suite under ~15 s: cases 01-05
...                 insert with the same expiresAt, case 06 sleeps past it and asserts.

Library             DateTime
Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource

Suite Setup         Setup Expiry And Ids
Suite Teardown      Delete Durable Entity


*** Variables ***
# Cases 01-05 (inserts + pre-expiry retrieves) must finish inside this window,
# and case 06 sleeps EXPIRY_SECONDS + 2 s margin. Raise it if a slow box flakes.
${EXPIRY_SECONDS}       ${6}


*** Test Cases ***
422_01_01 Create Transient Entity
    [Documentation]    5.6.1 Create Entity with a future expiresAt — served before expiry
    [Tags]    transient    4_22
    ${entity}=    Transient Building    ${eid_create}
    ${response}=    Create Entity From JSON-LD Content    ${entity}
    Check Response Status Code    201    ${response.status_code}
    Entity Is Retrievable    ${eid_create}

422_01_02 Batch Create Transient Entity
    [Documentation]    5.6.7 Batch Entity Creation with expiresAt
    [Tags]    transient    4_22
    ${entity}=    Transient Building    ${eid_batch_create}
    ${response}=    Batch Create Entities    ${entity}
    Check Response Status Code    201    ${response.status_code}
    Entity Is Retrievable    ${eid_batch_create}

422_01_03 Batch Upsert Transient Entity
    [Documentation]    5.6.8 Batch Entity Creation or Update with expiresAt
    [Tags]    transient    4_22
    ${entity}=    Transient Building    ${eid_batch_upsert}
    ${response}=    Batch Upsert Entities    ${entity}
    Check Response Status Code    201    ${response.status_code}
    Entity Is Retrievable    ${eid_batch_upsert}

422_01_04 Merge Makes Entity Transient
    [Documentation]    5.6.17 Merge Entity adding expiresAt to a durable entity
    [Tags]    transient    4_22
    ${entity}=    Durable Building    ${eid_merge}
    ${response}=    Create Entity From JSON-LD Content    ${entity}
    Check Response Status Code    201    ${response.status_code}
    &{headers}=    Create Dictionary    Content-Type=application/ld+json
    ${fragment}=    Evaluate
    ...    {"id": $eid_merge, "type": "Building", "expiresAt": $expiry, "@context": $ngsild_test_suite_context}
    ${response}=    PATCH
    ...    url=${url}/${ENTITIES_ENDPOINT_PATH}${eid_merge}
    ...    json=${fragment}
    ...    headers=${headers}
    ...    expected_status=any
    Check Response Status Code    204    ${response.status_code}
    Entity Is Retrievable    ${eid_merge}

422_01_05 Append Transient Attribute To Durable Entity
    [Documentation]    5.6.3 Append Attributes carrying an instance-level expiresAt —
    ...    the attribute expires, the entity stays
    [Tags]    transient    4_22
    ${entity}=    Durable Building    ${eid_append}
    ${response}=    Create Entity From JSON-LD Content    ${entity}
    Check Response Status Code    201    ${response.status_code}
    &{headers}=    Create Dictionary    Content-Type=application/ld+json
    ${fragment}=    Evaluate
    ...    {"temperature": {"type": "Property", "value": 21, "expiresAt": $expiry}, "@context": $ngsild_test_suite_context}
    ${response}=    POST
    ...    url=${url}/${ENTITIES_ENDPOINT_PATH}${eid_append}/attrs/
    ...    json=${fragment}
    ...    headers=${headers}
    ...    expected_status=any
    Check Response Status Code    204    ${response.status_code}
    ${response}=    Retrieve Entity    id=${eid_append}    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response.status_code}
    ${temp}=    Evaluate    $response.json().get("temperature", {})
    Should Be Equal As Numbers    ${temp["value"]}    21    appended transient attribute served with its value

422_01_06 Expired Entities And Attributes Are Gone
    [Documentation]    Past expiresAt every transient entity 404s and the transient
    ...    attribute instance is absent from its (still-alive) durable entity
    [Tags]    transient    4_22
    Sleep    ${EXPIRY_SECONDS + 2}s
    FOR    ${id}    IN    ${eid_create}    ${eid_batch_create}    ${eid_batch_upsert}    ${eid_merge}
        ${response}=    Retrieve Entity    id=${id}    context=${ngsild_test_suite_context}
        Check Response Status Code    404    ${response.status_code}
    END
    ${response}=    Retrieve Entity    id=${eid_append}    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response.status_code}
    ${keys}=    Evaluate    sorted($response.json().keys())
    Should Not Contain    ${keys}    temperature    the expired attribute member is absent from the parsed entity


*** Keywords ***
Setup Expiry And Ids
    ${now}=    Get Current Date    time_zone=UTC
    ${expiry}=    Add Time To Date    ${now}    ${EXPIRY_SECONDS}s    result_format=%Y-%m-%dT%H:%M:%SZ
    Set Suite Variable    ${expiry}
    FOR    ${name}    IN    eid_create    eid_batch_create    eid_batch_upsert    eid_merge    eid_append
        ${id}=    Generate Random Building Entity Id
        Set Suite Variable    ${${name}}    ${id}
    END

Transient Building
    [Arguments]    ${entity_id}
    ${entity}=    Evaluate
    ...    {"id": $entity_id, "type": "Building", "name": {"type": "Property", "value": "transient"}, "expiresAt": $expiry, "@context": $ngsild_test_suite_context}
    RETURN    ${entity}

Durable Building
    [Arguments]    ${entity_id}
    ${entity}=    Evaluate
    ...    {"id": $entity_id, "type": "Building", "name": {"type": "Property", "value": "durable"}, "@context": $ngsild_test_suite_context}
    RETURN    ${entity}

Entity Is Retrievable
    [Arguments]    ${entity_id}
    ${response}=    Retrieve Entity    id=${entity_id}    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response.status_code}

Delete Durable Entity
    Delete Entity    ${eid_append}
