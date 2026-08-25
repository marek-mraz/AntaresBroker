*** Settings ***
Documentation       Check that entities can be queried with two levels linked entities and pick parameter

Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Suite Setup         Create Initial Entities And Linked Entities
Suite Teardown      Delete Created Entities And Linked Entities
Test Template       Query Entities With Joins And Pick


*** Variables ***
${linking_entity_filename}=             building-relationship.jsonld
${level_1_linked_entity_filename}=      city-simple-attributes.jsonld
${level_2_linked_entity_filename}=      country-simple-attributes.jsonld


*** Test Cases ***    JOIN    PICK    EXPECTATION_FILENAME
019_22_01 Query Inline With Pick On Linked Entity
    [Documentation]    Check that entities can be queried with inline join and pick on linked entity
    [Tags]    e-query    5_7_2    4_5_23    4_21    since_v1.8.1
    inline    id,locatedAt{id,name}    linked-entity-retrieval/buildings-two-levels-inline-pick-id-locatedAt-019-22.json
019_22_02 Query Flat With Pick On Linked Entity
    [Documentation]    Check that entities can be queried with flat join and pick on linked entity
    [Tags]    e-query    5_7_2    4_5_23    4_21    since_v1.8.1
    flat    id,locatedAt{id,name}    linked-entity-retrieval/buildings-two-levels-flat-pick-id-locatedAt-019-22.json
019_22_03 Query Inline With Pick On Second Level Linked Entity
    [Documentation]    Check that entities can be queried with inline join and pick on 2nd level linked entity
    [Tags]    e-query    5_7_2    4_5_23    4_21    since_v1.8.1
    inline    id,locatedAt{id,name,isInCountry{id,description}}    linked-entity-retrieval/buildings-two-levels-inline-pick-id-locatedAt-isInCountry-019-22.json
019_22_04 Query Flat With Pick On Second Level Linked Entity
    [Documentation]    Check that entities can be queried with flat join and pick on 2nd level linked entity
    [Tags]    e-query    5_7_2    4_5_23    4_21    since_v1.8.1
    flat    id,locatedAt{id,name,isInCountry{id,description}}    linked-entity-retrieval/buildings-two-levels-flat-pick-id-locatedAt-isInCountry-019-22.json


*** Keywords ***
Query Entities With Joins And Pick
    [Documentation]    Check that entities can be queried with two levels linked entities and pick parameter
    [Arguments]    ${join}    ${pick}    ${expectation_filename}

    ${response}=    Query Entities
    ...    entity_types=Building
    ...    join=${join}
    ...    joinLevel=2
    ...    pick=${pick}
    ...    local=true
    ...    context=${ngsild_test_suite_context}

    Check Response Status Code    200    ${response.status_code}
    Check Response Body Content
    ...    expectation_filename=${expectation_filename}
    ...    response_body=${response.json()}

Create Initial Entities And Linked Entities
    ${entity_1_id}=    Catenate    ${building_id_prefix}019-22-1
    Set Suite Variable    ${entity_1_id}
    ${linked_entity_1_level_1_id}=    Catenate    ${CITY_ID_PREFIX}019-22-1
    Set Suite Variable    ${linked_entity_1_level_1_id}
    ${create_response1}=    Create Linking Entity
    ...    entity_filename=${linking_entity_filename}
    ...    entity_id=${entity_1_id}
    ...    linked_entity_id=${linked_entity_1_level_1_id}
    ${linked_entity_1_level_2_id}=    Catenate    ${COUNTRY_ID_PREFIX}019-22-1
    Set Suite Variable    ${linked_entity_1_level_2_id}
    ${create_response2}=    Create Linking Entity
    ...    entity_filename=${level_1_linked_entity_filename}
    ...    entity_id=${linked_entity_1_level_1_id}
    ...    linked_entity_id=${linked_entity_1_level_2_id}
    Check Response Status Code    201    ${create_response2.status_code}
    ${create_response3}=    Create Entity Selecting Content Type
    ...    filename=${level_2_linked_entity_filename}
    ...    entity_id=${linked_entity_1_level_2_id}
    ...    content_type=${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${create_response3.status_code}

    ${entity_2_id}=    Catenate    ${building_id_prefix}019-22-2
    Set Suite Variable    ${entity_2_id}
    ${linked_entity_2_level_1_id}=    Catenate    ${CITY_ID_PREFIX}019-22-2
    Set Suite Variable    ${linked_entity_2_level_1_id}
    ${create_response4}=    Create Linking Entity
    ...    entity_filename=${linking_entity_filename}
    ...    entity_id=${entity_2_id}
    ...    linked_entity_id=${linked_entity_2_level_1_id}
    ${linked_entity_2_level_2_id}=    Catenate    ${COUNTRY_ID_PREFIX}019-22-2
    Set Suite Variable    ${linked_entity_2_level_2_id}
    ${create_response5}=    Create Linking Entity
    ...    entity_filename=${level_1_linked_entity_filename}
    ...    entity_id=${linked_entity_2_level_1_id}
    ...    linked_entity_id=${linked_entity_2_level_2_id}
    Check Response Status Code    201    ${create_response2.status_code}
    ${create_response6}=    Create Entity Selecting Content Type
    ...    filename=${level_2_linked_entity_filename}
    ...    entity_id=${linked_entity_2_level_2_id}
    ...    content_type=${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${create_response3.status_code}

Delete Created Entities And Linked Entities
    Delete Entity    ${entity_1_id}
    Delete Entity    ${linked_entity_1_level_1_id}
    Delete Entity    ${linked_entity_1_level_2_id}
    Delete Entity    ${entity_2_id}
    Delete Entity    ${linked_entity_2_level_1_id}
    Delete Entity    ${linked_entity_2_level_2_id}

Create Linking Entity
    [Arguments]    ${entity_filename}    ${entity_id}    ${linked_entity_id}
    ${entity_payload}=    Load JSON From File    ${EXECDIR}/data/entities/${entity_filename}
    ${entity}=    Update Value To JSON    ${entity_payload}    $.id    ${entity_id}
    ${entity}=    Update Value To JSON    ${entity}    $..object    ${linked_entity_id}
    ${response}=    Create Entity From JSON-LD Content
    ...    ${entity}
    Check Response Status Code    201    ${response.status_code}
    RETURN    ${response}
