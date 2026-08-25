*** Settings ***
Documentation       Four brokers are set up A, B, C and D. A has three registrations, one inclusive for the entity created in B, one redirect for the entity created in C and one redirect for the entity created in D. Check that the entity returned from C has the same location attribute found in A.

Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceDiscovery.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Setup Initial Context Source Registrations
Test Teardown       Delete Entities and Delete Registrations

*** Variables ***
${entity_payload_filename}                     interoperability/offstreet-parking1-location-and-name.jsonld
${first_full_entity_payload_filename}          interoperability/offstreet-parking1-full.jsonld
${second_full_entity_payload_filename}         interoperability/offstreet-parking2-full.jsonld
${inclusive_registration_payload_file_path}    csourceRegistrations/interoperability/context-source-registration-inclusive-2.jsonld
${redirect_registration_payload_file_path}     csourceRegistrations/interoperability/context-source-registration-redirect-2.jsonld
${b1_url}
${b2_url}
${b3_url}
${b4_url}

*** Test Cases ***
IOP_CNF_02_02 Retrieve OffStreetParking:1 Location Attribute
    [Documentation]    Pre-conditions: no user context. Data only on leaves. B contains OffStreetParking1. C contains OffStreetParking1 with location and name only. D contains OffStreetParking2.
    ...                Registrations established: Inclusive in A to B. Redirect in A to C. Redirect in A to D.
    [Tags]    since_v1.6.1    iop    4_3_3    cf_06    additive-inclusive    proxy-redirect    4_3_6    5_7_1

    #Client retrieves OffStreetParking:1 in A and checks for a partial successful. The entity returned should contain the location attribute.
    ${response}=    Retrieve Entity    ${entity_id}    broker_url=${b1_url}    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response.status_code}    # NGSI-LD 6.5.3.1: Retrieve Entity has no 207 response
    ${payload}=    Set To Dictionary    ${response.json()}
    Should Contain    ${payload}    location

    #Client retrieves OffStreetParking:1 in C with local=true.
    ${response}=    Retrieve Entity    ${entity_id}    broker_url=${b3_url}    local=true    context=${ngsild_test_suite_context}
    ${expected_payload}=    Set To Dictionary    ${response.json()}

    #Client checks that the location attribute in C is the same as the one in A.
    Should Be Equal    ${payload}[location][value]    ${expected_payload}[location][value]

*** Keywords ***
Setup Initial Context Source Registrations
    ${entity_id}=    Generate Random Parking Entity Id
    Set Suite Variable    ${entity_id}
    ${second_entity_id}=    Generate Random Parking Entity Id
    Set Suite Variable    ${second_entity_id}
    ${response}=    Create Entity    ${first_full_entity_payload_filename}    ${entity_id}    broker_url=${b2_url}
    Check Response Status Code    201    ${response.status_code}
    ${response}=    Create Entity    ${entity_payload_filename}    ${entity_id}    broker_url=${b3_url}
    Check Response Status Code    201    ${response.status_code}
    ${response}=    Create Entity    ${second_full_entity_payload_filename}    ${second_entity_id}    broker_url=${b4_url}
    Check Response Status Code    201    ${response.status_code}

    ${registration_id1}=     Generate Random CSR Id
    Set Suite Variable    ${registration_id1}
    ${registration_payload}=    Prepare Context Source Registration From File
    ...    ${registration_id1}
    ...    ${inclusive_registration_payload_file_path}
    ...    entity_id=${entity_id}
    ...    broker_url=${b2_url}
    ...    mode=inclusive
    ${response}=    Create Context Source Registration With Return    ${registration_payload}    broker_url=${b1_url}
    Check Response Status Code    201    ${response.status_code}

    ${registration_id2}=     Generate Random CSR Id
    Set Suite Variable    ${registration_id2}
    ${registration_payload}=    Prepare Context Source Registration From File
    ...    ${registration_id2}
    ...    ${redirect_registration_payload_file_path}
    ...    entity_id=${entity_id}
    ...    broker_url=${b3_url}
    ...    mode=redirect
    ${response}=    Create Context Source Registration With Return    ${registration_payload}    broker_url=${b1_url}
    Check Response Status Code    201    ${response.status_code}

    ${registration_id3}=     Generate Random CSR Id
    Set Suite Variable    ${registration_id3}
    ${registration_payload}=    Prepare Context Source Registration From File
    ...    ${registration_id3}
    ...    ${redirect_registration_payload_file_path}
    ...    entity_id=${second_entity_id}
    ...    broker_url=${b4_url}
    ...    mode=redirect
    ${response}=    Create Context Source Registration With Return    ${registration_payload}    broker_url=${b1_url}
    Check Response Status Code    201    ${response.status_code}

    # Registrations propagate asynchronously to the broker's in-VM registry cache — an
    # immediate query/create can race ahead of the last registration (flaky aux/inclusive merges).
    Sleep    1s

Delete Entities And Delete Registrations
    Delete Context Source Registration    ${registration_id1}    broker_url=${b1_url}
    Delete Context Source Registration    ${registration_id2}    broker_url=${b1_url}
    Delete Context Source Registration    ${registration_id3}    broker_url=${b1_url}
    Delete Entity    ${entity_id}    broker_url=${b2_url}
    Delete Entity    ${entity_id}    broker_url=${b3_url}
    Delete Entity    ${second_entity_id}    broker_url=${b4_url}
