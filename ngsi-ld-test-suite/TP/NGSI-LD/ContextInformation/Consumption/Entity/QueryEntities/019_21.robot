*** Settings ***
Documentation       Check that entities can be queried with linked entity retrieval and omit parameter

Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Suite Setup         Create Initial Entities And Linked Entities
Suite Teardown      Delete Created Entities And Linked Entities
Test Template       Query Entities With Joins And Omit


*** Variables ***
${linking_entity_filename}=     building-different-attributes-types.jsonld
${linked_entity_filename}=      city-simple-attributes.jsonld


*** Test Cases ***    JOIN    OMIT    EXPECTATION_FILENAME
019_21_01 Query Inline With Omit On Linking Entity
    [Documentation]    Check that entities can be queried with inline linked entity retrieval and omit on name
    [Tags]    e-query    5_7_2    4_5_23    4_21    since_v1.8.1
    inline    name    linked-entity-retrieval/buildings-one-level-inline-omit-name-019-21.json
019_21_02 Query Flat With Omit On Linking Entity
    [Documentation]    Check that entities can be queried with flat linked entity retrieval and omit on name
    [Tags]    e-query    5_7_2    4_5_23    4_21    since_v1.8.1
    flat    name    linked-entity-retrieval/buildings-one-level-flat-omit-name-019-21.json
019_21_03 Query Inline With Omit On Linked Entity
    [Documentation]    Check that entities can be queried with inline linked entity retrieval and omit on type
    [Tags]    e-query    5_7_2    4_5_23    4_21    since_v1.8.1
    inline    name,locatedAt{name}    linked-entity-retrieval/buildings-one-level-inline-omit-id-locatedAt-019-21.json
019_21_04 Query Flat With Omit On Linked Entity
    [Documentation]    Check that entities can be queried with flat linked entity retrieval and omit on type
    [Tags]    e-query    5_7_2    4_5_23    4_21    since_v1.8.1
    flat    name,locatedAt{name}    linked-entity-retrieval/buildings-one-level-flat-omit-id-locatedAt-019-21.json


*** Keywords ***
Query Entities With Joins And Omit
    [Documentation]    Check that entities can be queried with linked entity retrieval and omit parameter
    [Arguments]    ${join}    ${omit}    ${expectation_filename}

    ${response}=    Query Entities
    ...    entity_types=Building
    ...    join=${join}
    ...    joinLevel=1
    ...    omit=${omit}
    ...    local=true
    ...    context=${ngsild_test_suite_context}

    Check Response Status Code    200    ${response.status_code}
    Check Response Body Content
    ...    expectation_filename=${expectation_filename}
    ...    response_body=${response.json()}

Create Initial Entities And Linked Entities
    ${first_entity_id}=    Catenate    ${BUILDING_ID_PREFIX}019-21-1
    ${first_linked_entity_id}=    Catenate    ${CITY_ID_PREFIX}019-21-1
    Set Suite Variable    ${first_entity_id}
    Set Suite Variable    ${first_linked_entity_id}
    ${create_response1}=    Create Linking Entity
    ...    linking_entity_id=${first_entity_id}
    ...    linked_entity_id=${first_linked_entity_id}
    ${create_response2}=    Create Entity Selecting Content Type
    ...    filename=${linked_entity_filename}
    ...    entity_id=${first_linked_entity_id}
    ...    content_type=${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${create_response2.status_code}

    ${second_entity_id}=    Catenate    ${BUILDING_ID_PREFIX}019-21-2
    ${second_linked_entity_id}=    Catenate    ${CITY_ID_PREFIX}019-21-2
    Set Suite Variable    ${second_entity_id}
    Set Suite Variable    ${second_linked_entity_id}
    ${create_response3}=    Create Linking Entity
    ...    linking_entity_id=${second_entity_id}
    ...    linked_entity_id=${second_linked_entity_id}
    ${create_response4}=    Create Entity Selecting Content Type
    ...    filename=${linked_entity_filename}
    ...    entity_id=${second_linked_entity_id}
    ...    content_type=${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${create_response4.status_code}

Delete Created Entities And Linked Entities
    Delete Entity    ${first_entity_id}
    Delete Entity    ${first_linked_entity_id}
    Delete Entity    ${second_entity_id}
    Delete Entity    ${second_linked_entity_id}

Create Linking Entity
    [Arguments]    ${linking_entity_id}    ${linked_entity_id}
    ${entity_payload}=    Load JSON From File    ${EXECDIR}/data/entities/${linking_entity_filename}
    ${entity}=    Update Value To JSON    ${entity_payload}    $.id    ${linking_entity_id}
    ${entity}=    Update Value To JSON    ${entity}    $.locatedAt.object    ${linked_entity_id}
    ${response}=    Create Entity From JSON-LD Content
    ...    ${entity}
    Check Response Status Code    201    ${response.status_code}
    RETURN    ${response}
