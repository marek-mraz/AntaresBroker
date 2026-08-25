*** Settings ***
Documentation       Check that one can query temporal evolution of entities with orderBy

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Suite Setup         Setup Initial Entities
Suite Teardown      Delete Entities
Test Template       Query Temporal Entities With OrderBy


*** Variables ***
${first_id}=        ${BUILDING_ID_PREFIX}1
${second_id}=       ${BUILDING_ID_PREFIX}2
${third_id}=        ${BUILDING_ID_PREFIX}3
@{asc}=             ${first_id}    ${second_id}    ${third_id}
@{desc}=            ${third_id}    ${second_id}    ${first_id}
@{created}=         ${second_id}    ${first_id}    ${third_id}


*** Test Cases ***    ORDER_BY    EXPECTED_ORDER
021_23_01 QueryWithOrderByAttributeIsRejected
    [Documentation]    5.7.4.4: "If the ordering parameter is present and
    ...    refers an entity name other than id, then an error of type
    ...    BadRequestData shall be raised."
    [Tags]    te-query    5_7_4    4_23    since_v1.9.1
    [Template]    Query Temporal Entities With Rejected OrderBy
    name
021_23_02 QueryWithOrderById
    [Documentation]    Check that one can query entities with orderBy on id
    [Tags]    te-query    5_7_4    4_23    since_v1.9.1
    id    ${asc}
021_23_03 QueryWithOrderByIdDescending
    [Documentation]    5.7.4.4 allows ordering by id; 4.23 direction applies
    [Tags]    te-query    5_7_4    4_23    since_v1.9.1
    id;desc    ${desc}
021_23_04 QueryWithOrderByCreatedAtIsRejected
    [Documentation]    5.7.4.4: system members other than id are rejected on
    ...    the temporal query
    [Tags]    te-query    5_7_4    4_23    since_v1.9.1
    [Template]    Query Temporal Entities With Rejected OrderBy
    createdAt
021_23_05 QueryWithOrderBySubAttributeIsRejected
    [Documentation]    5.7.4.4: attribute paths are rejected on the temporal
    ...    query
    [Tags]    te-query    5_7_4    4_23    since_v1.9.1
    [Template]    Query Temporal Entities With Rejected OrderBy
    name.subProperty
021_23_06 QueryWithMultipleOrderByIsRejected
    [Documentation]    5.7.4.4: every ordering member must be id — a list
    ...    containing any other member is rejected
    [Tags]    te-query    5_7_4    4_23    since_v1.9.1
    [Template]    Query Temporal Entities With Rejected OrderBy
    id,name


*** Keywords ***
Query Temporal Entities With Rejected OrderBy
    [Documentation]    5.7.4.4: temporal ordering may only refer to "id"
    [Arguments]    ${orderBy}
    ${response}=    Query Temporal Representation Of Entities
    ...    entity_types=Building
    ...    orderBy=${orderBy}
    ...    timerel=after
    ...    timeAt=1970-01-01T00:00:00Z
    ...    timeproperty=createdAt
    ...    context=${ngsild_test_suite_context}
    Check Response Status Code    400    ${response.status_code}

Query Temporal Entities With OrderBy
    [Documentation]    Query temporal evolution of entities giving an orderBy
    [Arguments]    ${orderBy}    ${expected_order}

    ${response}=    Query Temporal Representation Of Entities
    ...    entity_types=Building
    ...    orderBy=${orderBy}
    ...    timerel=after
    ...    timeAt=1970-01-01T00:00:00Z
    ...    timeproperty=createdAt
    ...    context=${ngsild_test_suite_context}

    Check Response Status Code    200    ${response.status_code}

    Check Response Body Containing Entities URIS set to
    ...    ${expected_order}
    ...    ${response.json()}
    ...    ignore_order=False

Setup Initial Entities
    ${create_response2}=    Create Entity Selecting Content Type
    ...    orderBy/building_2.jsonld
    ...    ${second_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${create_response2.status_code}
    ${create_response1}=    Create Entity Selecting Content Type
    ...    orderBy/building_1.jsonld
    ...    ${first_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${create_response1.status_code}
    ${create_response3}=    Create Entity Selecting Content Type
    ...    orderBy/building_3.jsonld
    ...    ${third_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${create_response3.status_code}

Delete Entities
    Delete Entity    ${first_id}
    Delete Entity    ${second_id}
    Delete Entity    ${third_id}
