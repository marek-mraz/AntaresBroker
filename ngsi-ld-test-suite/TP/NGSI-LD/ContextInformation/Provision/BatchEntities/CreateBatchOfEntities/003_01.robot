*** Settings ***
Documentation       Check that one can create a batch of entities

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Teardown       Delete Initial Entities
Test Template       Batch Create Entity Scenarios


*** Test Cases ***    FILENAME
003_01_01 MinimalEntity
    [Tags]    be-create    5_6_7
    building-minimal.jsonld
003_01_02 EntityWithSimpleProperties
    [Tags]    be-create    5_6_7
    building-simple-attributes.jsonld
003_01_03 EntityWithSimpleRelationships
    [Tags]    be-create    5_6_7
    building-relationship.jsonld
003_01_04 EntityWithRelationshipsProperties
    [Tags]    be-create    5_6_7
    building-relationship-of-property.jsonld


*** Keywords ***
Batch Create Entity Scenarios
    [Documentation]    Check that one can create a batch of entities
    [Arguments]    ${filename}
    ${first_entity_id}=    Generate Random Building Entity Id
    ${second_entity_id}=    Generate Random Building Entity Id
    ${first_entity}=    Load Entity    ${filename}    ${first_entity_id}
    ${second_entity}=    Load Entity    ${filename}    ${second_entity_id}
    @{entities_to_be_created}=    Create List    ${first_entity}    ${second_entity}
    @{expected_entities_ids}=    Create List    ${first_entity_id}    ${second_entity_id}
    Set Test Variable    @{expected_entities_ids}

    ${response}=    Batch Create Entities
    ...    @{entities_to_be_created}

    Check Response Status Code    201    ${response.status_code}
    ${entities_to_be_queried}=    Catenate    SEPARATOR=,    ${first_entity_id}    ${second_entity_id}
    Check Response Body Containing Array Of URIs set to    ${expected_entities_ids}    ${response.json()}
    ${response1}=    Query Entities
    ...    entity_ids=${entities_to_be_queried}
    ...    entity_types=Building
    ...    context=${ngsild_test_suite_context}
    ...    accept=${CONTENT_TYPE_LD_JSON}
    Check Created Resources Set To
    ...    expected_resources=${entities_to_be_created}
    ...    response_body=${response1.json()}

Delete Initial Entities
    Batch Delete Entities    entities_ids_to_be_deleted=@{expected_entities_ids}
