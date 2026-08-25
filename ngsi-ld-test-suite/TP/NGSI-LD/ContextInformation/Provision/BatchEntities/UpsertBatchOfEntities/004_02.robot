*** Settings ***
Documentation       Check that one can upsert a batch of non-existing and existing entities where non-existing will be created and existing will be replaced

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Teardown       Delete Initial Entities
Test Template       Batch Upsert Non-existing And Existing Entities Scenarios


*** Test Cases ***    FILENAME
004_02_01 EntityWithSimpleProperties
    [Tags]    be-upsert    5_6_8
    building-simple-attributes.jsonld
004_02_02 EntityWithSimpleRelationships
    [Tags]    be-upsert    5_6_8
    building-relationship.jsonld
004_02_03 EntityWithRelationshipsProperties
    [Tags]    be-upsert    5_6_8
    building-relationship-of-property.jsonld


*** Keywords ***
Batch Upsert Non-existing And Existing Entities Scenarios
    [Documentation]    Check that one can upsert a batch of non-existing and existing entities
    [Arguments]    ${filename}
    Prepare Entities To Upsert    ${filename}

    ${response}=    Batch Upsert Entities    @{entities_to_be_upserted}

    @{expected_entities_ids}=    Create List    ${new_entity_id}
    Check Response Status Code    201    ${response.status_code}
    Check Response Body Containing Array Of URIs set to    ${expected_entities_ids}    ${response.json()}
    @{upserted_entities_ids}=    Create List
    ...    ${new_entity_id}
    ...    ${first_existing_entity_id}
    ...    ${second_existing_entity_id}
    ${expected_updated_entities_ids}=    Catenate    SEPARATOR=,    @{upserted_entities_ids}
    ${response1}=    Query Entities
    ...    entity_ids=${expected_updated_entities_ids}
    ...    entity_types=Building
    ...    context=${ngsild_test_suite_context}
    ...    accept=${CONTENT_TYPE_LD_JSON}
    Check Updated Resources Set To    ${entities_to_be_upserted}    ${response1.json()}

Prepare Entities To Upsert
    [Arguments]    ${filename}
    ${first_existing_entity_id}=    Generate Random Building Entity Id
    ${second_existing_entity_id}=    Generate Random Building Entity Id
    Create Entity    building-minimal.jsonld    ${first_existing_entity_id}
    Create Entity    building-minimal.jsonld    ${second_existing_entity_id}
    Set Test Variable    ${first_existing_entity_id}
    Set Test Variable    ${second_existing_entity_id}
    ${new_entity_id}=    Generate Random Building Entity Id
    Set Test Variable    ${new_entity_id}
    ${new_entity}=    Load Entity    ${filename}    ${new_entity_id}
    ${first_existing_entity}=    Load Entity    ${filename}    ${first_existing_entity_id}
    ${second_existing_entity}=    Load Entity    ${filename}    ${second_existing_entity_id}
    @{entities_to_be_upserted}=    Create List
    ...    ${new_entity}
    ...    ${first_existing_entity}
    ...    ${second_existing_entity}
    Set Test Variable    ${entities_to_be_upserted}

Delete Initial Entities
    @{entities_ids_to_be_deleted}=    Create List
    ...    ${first_existing_entity_id}
    ...    ${second_existing_entity_id}
    ...    ${new_entity_id}
    Batch Delete Entities    entities_ids_to_be_deleted=@{entities_ids_to_be_deleted}
