*** Settings ***
Documentation       Check that one can query entities with orderBy

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Suite Setup         Setup Initial Entities
Suite Teardown      Delete Entities
Test Template       Query Entities With OrderBy


*** Variables ***
${first_id}=        ${BUILDING_ID_PREFIX}1
${second_id}=       ${BUILDING_ID_PREFIX}2
${third_id}=        ${BUILDING_ID_PREFIX}3
@{asc}=             ${first_id}    ${second_id}    ${third_id}
@{desc}=            ${third_id}    ${second_id}    ${first_id}
@{created}=         ${second_id}    ${first_id}    ${third_id}


*** Test Cases ***    ORDER_BY    EXPECTED_ORDER
019_19_01 QueryWithOrderByAttribute
    [Documentation]    Check that one can query entities with orderBy on an attribute
    [Tags]    e-query    5_7_2    4_23    since_v1.9.1
    name    ${asc}
019_19_02 QueryWithOrderById
    [Documentation]    Check that one can query entities with orderBy on id
    [Tags]    e-query    5_7_2    4_23    since_v1.9.1
    id    ${asc}
019_19_03 QueryWithOrderByCreatedAt
    [Documentation]    Check that one can query entities with orderBy on createdAt
    [Tags]    e-query    5_7_2    4_23    since_v1.9.1
    createdAt    ${created}
019_19_04 QueryWithOrderByNameCreatedAt
    [Documentation]    Check that one can query entities with orderBy on an attribute createdAt
    [Tags]    e-query    5_7_2    4_23    since_v1.9.1
    name.createdAt    ${created}
019_19_05 QueryWithOrderBySubAttribute
    [Documentation]    Check that one can query entities with orderBy an attribute subproperty
    [Tags]    e-query    5_7_2    4_23    since_v1.9.1
    name.subProperty    ${asc}
019_19_06 QueryWithOrderByAscending
    [Documentation]    Check that one can query entities with orderBy on a name ascending
    [Tags]    e-query    5_7_2    4_23    since_v1.9.1
    name;asc    ${asc}
019_19_07 QueryWithOrderByDescending
    [Documentation]    Check that one can query entities with orderBy on a name descending
    [Tags]    e-query    5_7_2    4_23    since_v1.9.1
    name;desc    ${desc}
019_19_08 QueryWithMultipleOrderBy
    [Documentation]    Check that one can query entities with orderBy on multiple members
    [Tags]    e-query    5_7_2    4_23    since_v1.9.1
    type,name    ${asc}
019_19_09 QueryWithComplexOrderBy
    [Documentation]    Check that one can query entities with a complex orderBy
    [Tags]    e-query    5_7_2    4_23    since_v1.9.1
    type;asc,name.subProperty;desc,name;asc    ${desc}


*** Keywords ***
Query Entities With OrderBy
    [Documentation]    Query entities giving an orderBy
    [Arguments]    ${orderBy}    ${expected_order}

    ${response}=    Query Entities
    ...    entity_types=Building
    ...    orderBy=${orderBy}
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
