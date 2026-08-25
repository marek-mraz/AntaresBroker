*** Settings ***
Documentation       Check that an entity can be retrieved with two levels linked entities and omit parameter

Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Suite Setup         Create Initial Entity And Linked Entities
Suite Teardown      Delete Created Entity And Linked Entities
Test Template       Retrieve Entity With Joins And Omit


*** Variables ***
${linking_entity_filename}=             building-relationship.jsonld
${level_1_linked_entity_filename}=      city-simple-attributes.jsonld
${level_2_linked_entity_filename}=      country-simple-attributes.jsonld


*** Test Cases ***    JOIN    OMIT    EXPECTATION_FILENAME
018_21_01 Retrieve Inline With Omit On Linked Entity
    [Documentation]    Check that an entity can be retrieved with inline join and omit on linked entity
    [Tags]    e-retrieve    5_7_1    4_5_23    4_21    since_v1.8.1
    inline    type,locatedAt{name}    linked-entity-retrieval/building-two-levels-inline-omit-id-locateAt-018-21.json
018_21_02 Retrieve Flat With Omit On Linked Entity
    [Documentation]    Check that an entity can be retrieved with flat join and omit on linked entity
    [Tags]    e-retrieve    5_7_1    4_5_23    4_21    since_v1.8.1
    flat    type,locatedAt{name}    linked-entity-retrieval/building-two-levels-flat-omit-id-locateAt-018-21.json
018_21_03 Retrieve Inline With Omit On Second Level Linked Entity
    [Documentation]    Check that an entity can be retrieved with inline join and omit on 2nd level linked entity
    [Tags]    e-retrieve    5_7_1    4_5_23    4_21    since_v1.8.1
    inline    type,locatedAt{name,isInCountry{description}}    linked-entity-retrieval/building-two-levels-inline-omit-id-locateAt-isInCountry-018-21.json
018_21_04 Retrieve Flat With Omit On Second Level Linked Entity
    [Documentation]    Check that an entity can be retrieved with flat join and omit on 2nd level linked entity
    [Tags]    e-retrieve    5_7_1    4_5_23    4_21    since_v1.8.1
    flat    type,locatedAt{name,isInCountry{description}}    linked-entity-retrieval/building-two-levels-flat-omit-id-locateAt-isInCountry-018-21.json


*** Keywords ***
Retrieve Entity With Joins And Omit
    [Documentation]    Check that an entity can be retrieved with two levels linked entities and omit parameter
    [Arguments]    ${join}    ${omit}    ${expectation_filename}

    ${response}=    Retrieve Entity
    ...    id=${linking_entity_id}
    ...    join=${join}
    ...    joinLevel=2
    ...    omit=${omit}
    ...    local=true
    ...    context=${ngsild_test_suite_context}

    Check Response Status Code    200    ${response.status_code}
    Check Response Body Content
    ...    expectation_filename=${expectation_filename}
    ...    response_body=${response.json()}

Create Initial Entity And Linked Entities
    ${linking_entity_id}=    Catenate    ${BUILDING_ID_PREFIX}018-21
    Set Suite Variable    ${linking_entity_id}
    ${response}=    Create Entity Selecting Content Type
    ...    ${linking_entity_filename}
    ...    ${linking_entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${response.status_code}

    ${level_1_linked_entity_id}=    Catenate    ${CITY_ID_PREFIX}Paris
    Set Suite Variable    ${level_1_linked_entity_id}
    ${response}=    Create Entity Selecting Content Type
    ...    ${level_1_linked_entity_filename}
    ...    ${level_1_linked_entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${response.status_code}

    ${level_2_linked_entity_id}=    Catenate    ${COUNTRY_ID_PREFIX}France
    Set Suite Variable    ${level_2_linked_entity_id}
    ${response}=    Create Entity Selecting Content Type
    ...    ${level_2_linked_entity_filename}
    ...    ${level_2_linked_entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${response.status_code}

Delete Created Entity And Linked Entities
    Delete Entity    ${linking_entity_id}
    Delete Entity    ${level_1_linked_entity_id}
    Delete Entity    ${level_2_linked_entity_id}
