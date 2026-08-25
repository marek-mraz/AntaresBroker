*** Settings ***
Documentation       Check that one can append entity attributes with noOverwrite option

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Create Initial Entity
Test Teardown       Delete Initial Entity
Test Template       Append Attributes With Params


*** Variables ***
${filename}=                        vehicle-speed-two-datasetid.jsonld
${existing_attribute_name}=         https://ngsi-ld-test-suite/context#speed
${non_existing_attribute_name}=     https://uri.etsi.org/ngsi-ld/default-context/attribute_to_be_added


*** Test Cases ***    STATUS_CODE    FRAGMENT_FILENAME    EXPECTATION_FILENAME
010_04_01 Append Entity Attributes And Ignore Existing Multi-attribute Instance
    207    vehicle-attribute-to-add-fragment.jsonld    vehicle-speed-appended.jsonld
010_04_02 Append Entity Attributes With A New Multi-attribute Instance
    204    vehicle-speed-different-datasetid-fragment.jsonld    vehicle-speed-different-datasetid.jsonld


*** Keywords ***
Append Attributes With Params
    [Documentation]    Check that one can append entity attributes with noOverwrite option
    [Tags]    ea-append    5_6_3
    [Arguments]    ${status_code}    ${fragment_filename}    ${expectation_filename}
    ${response}=    Append Entity Attributes With Parameters
    ...    ${entity_id}
    ...    ${fragment_filename}
    ...    ${CONTENT_TYPE_LD_JSON}
    ...    noOverwrite
    Check Response Status Code    ${status_code}    ${response.status_code}
    IF    ${status_code} == 207
        @{expected_successful_attributes_names}=    Create List    ${non_existing_attribute_name}
        @{expected_failed_attributes_names}=    Create List    ${existing_attribute_name}
        &{expected_update_result}=    Create Update Result
        ...    ${expected_successful_attributes_names}
        ...    ${expected_failed_attributes_names}
        Check Response Body Containing Update Result    ${expected_update_result}    ${response.json()}
    END

    ${entity_expectation_payload}=    Load Test Sample    entities/expectations/${expectation_filename}    ${entity_id}
    ${response1}=    Retrieve Entity
    ...    id=${entity_id}
    ...    context=${ngsild_test_suite_context}
    ...    accept=${CONTENT_TYPE_LD_JSON}
    ${ignored_attributes}=    Create List    @context
    Check Updated Resource Set To
    ...    updated_resource=${entity_expectation_payload}
    ...    response_body=${response1.json()}
    ...    ignored_keys=${ignored_attributes}

Create Initial Entity
    ${entity_id}=    Generate Random Vehicle Entity Id
    ${response}=    Create Entity Selecting Content Type
    ...    ${filename}
    ...    ${entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${response.status_code}
    Set Test Variable    ${entity_id}

Delete Initial Entity
    Delete Entity    ${entity_id}
