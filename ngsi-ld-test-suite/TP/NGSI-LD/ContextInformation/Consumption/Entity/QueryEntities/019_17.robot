*** Settings ***
Documentation       Check that one can query entities with pick and omit query params

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Suite Setup         Setup Initial Entities
Suite Teardown      Delete Entities
Test Template       Query Entities With Pick Or Omit Query Params


*** Test Cases ***    PICK    OMIT    EXPECTED_FILENAME
019_17_01 QueryWithPickOnAttributes
    [Documentation]    Check that one can query entities with pick on attributes
    [Tags]    e-query    5_7_2    4_21    since_v1.8.1
    name    ${EMPTY}    pick-omit/entities-pick-name.json
019_17_02 QueryWithOmitOnAttributes
    [Documentation]    Check that one can query entities with omit on attributes
    [Tags]    e-query    5_7_2    4_21    since_v1.8.1
    ${EMPTY}    name    pick-omit/entities-omit-name.json
019_17_03 QueryWithPickOnNonCoreMembers
    [Documentation]    Check that one can query entities with pick on core members
    [Tags]    e-query    5_7_2    4_21    since_v1.8.1
    id    ${EMPTY}    pick-omit/entities-pick-id.json
019_17_04 QueryWithOmitOnNonCoreMembers
    [Documentation]    Check that one can query entities with omit on core members
    [Tags]    e-query    5_7_2    4_21    since_v1.8.1
    ${EMPTY}    id    pick-omit/entities-omit-id.json
019_17_05 QueryWithPickOnAttributesAndCoreMembers
    [Documentation]    Check that one can query entities with pick on attributes and core members
    [Tags]    e-query    5_7_2    4_21    since_v1.8.1
    id,name    ${EMPTY}    pick-omit/entities-pick-id-name.json
019_17_06 QueryWithOmitOnAttributesAndCoreMembers
    [Documentation]    Check that one can query entities with omit on attributes and core members
    [Tags]    e-query    5_7_2    4_21    since_v1.8.1
    ${EMPTY}    type,name    pick-omit/entities-omit-type-name.json


*** Keywords ***
Query Entities With Pick Or Omit Query Params
    [Documentation]    Query entities giving pick or omit query params different values
    [Arguments]    ${pick}    ${omit}    ${expectation_filename}

    ${response}=    Query Entities
    ...    entity_types=Building,Vehicle
    ...    pick=${pick}
    ...    omit=${omit}
    ...    context=${ngsild_test_suite_context}

    Check Response Status Code    200    ${response.status_code}
    # ignore order since responses do not all have an id member
    Check Response Body Content    ${expectation_filename}    ${response.json()}    ignore_order=True

Setup Initial Entities
    ${first_entity_id}=    Catenate    ${BUILDING_ID_PREFIX}019-17-1
    Set Suite Variable    ${first_entity_id}
    ${create_response1}=    Create Entity Selecting Content Type
    ...    building-simple-attributes.jsonld
    ...    ${first_entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${create_response1.status_code}

    ${second_entity_id}=    Catenate    ${VEHICLE_ID_PREFIX}019-17-2
    Set Suite Variable    ${second_entity_id}
    ${create_response2}=    Create Entity Selecting Content Type
    ...    vehicle-simple-attributes.jsonld
    ...    ${second_entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${create_response2.status_code}

    ${third_entity_id}=    Catenate    ${BUILDING_ID_PREFIX}019-17-3
    Set Suite Variable    ${third_entity_id}
    ${create_response3}=    Create Entity Selecting Content Type
    ...    building-simple-attributes-third.jsonld
    ...    ${third_entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${create_response3.status_code}

Delete Entities
    Delete Entity    ${first_entity_id}
    Delete Entity    ${second_entity_id}
    Delete Entity    ${third_entity_id}
